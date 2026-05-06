import { useCallback, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import type {
  CleanupPlan,
  CleanupProvider,
  PlanId,
  PlannedOperation,
  RiskLevel,
  RiskMax,
} from '@emailibrium/types';
import { getPlan } from '@emailibrium/api';
import { usePlanOperations } from '../hooks/usePlanOperations';
import { RiskLegend } from './RiskLegend';
import { PlanWarningsBanner } from './PlanWarningsBanner';
import { AccountSummaryTile } from './AccountSummaryTile';
import { PlanDiffGroup } from './PlanDiffGroup';
import { RiskAcknowledger } from './RiskAcknowledger';
import { ApplyButtons } from './ApplyButtons';
import { RefreshAccountAffordance } from './RefreshAccountAffordance';
import { sourceKey, sourceLabel } from './groupKey';
import { useCleanupApply } from '../hooks/useCleanupApply';
import { useCleanupTelemetry } from '../hooks/useCleanupTelemetry';
import { CleanupProgress } from '../CleanupProgress';

export interface CleanupReviewProps {
  planId: PlanId;
  userId: string;
  onCancel(): void;
  /**
   * Phase D: when true, render the plan + operations as read-only — Apply
   * buttons, RiskAcknowledger, and stale-age refresh affordances are hidden.
   * Used by the plan-history detail route. Defaults to false to preserve
   * the wizard's existing behaviour.
   */
  readOnly?: boolean;
}

const STALE_MS = 25 * 60 * 1000;

interface AccountAggregate {
  accountId: string;
  counts: {
    archive?: number;
    addLabel?: number;
    move?: number;
    delete?: number;
    unsubscribe?: number;
    markRead?: number;
    star?: number;
  };
  risk: { low: number; medium: number; high: number };
  groups: Map<
    string,
    {
      sourceKey: string;
      sourceLabel: string;
      rows: PlannedOperation[];
      risk: RiskLevel;
    }
  >;
}

function highestRisk(rows: PlannedOperation[]): RiskLevel {
  let max: RiskLevel = 'low';
  for (const r of rows) {
    if (r.risk === 'high') return 'high';
    if (r.risk === 'medium') max = 'medium';
  }
  return max;
}

function detectProvider(plan: CleanupPlan, accountId: string): CleanupProvider {
  // Phase D: closes the prior TODO that hardcoded 'gmail'. Reads the
  // backend-populated `accountProviders` map; falls back to 'gmail' only
  // for older cached plans missing the new field.
  return plan.accountProviders?.[accountId] ?? 'gmail';
}

export function CleanupReview({ planId, userId, onCancel, readOnly = false }: CleanupReviewProps) {
  // Phase D telemetry — emits cleanup_plan_reviewed on unmount or apply.
  const telemetry = useCleanupTelemetry(planId);
  const planQuery = useQuery<CleanupPlan>({
    queryKey: ['cleanup', 'plan', planId, userId],
    queryFn: () => getPlan(userId, planId),
    enabled: Boolean(planId && userId),
    staleTime: 30_000,
  });

  const opsResult = usePlanOperations(planId, userId, { pageSize: 200 });

  const [ackedHighSeqs, setAckedHighSeqs] = useState<number[]>([]);
  const [ackedMediumGroupKeys, setAckedMediumGroupKeys] = useState<string[]>([]);
  const [refreshedAccounts, setRefreshedAccounts] = useState<Set<string>>(() => new Set());

  const handleAcksChange = useCallback((highSeqs: number[], mediumKeys: string[]) => {
    setAckedHighSeqs(highSeqs);
    setAckedMediumGroupKeys(mediumKeys);
  }, []);

  const ackedHighSet = useMemo(() => new Set(ackedHighSeqs), [ackedHighSeqs]);
  const ackedMediumSet = useMemo(() => new Set(ackedMediumGroupKeys), [ackedMediumGroupKeys]);

  const toggleHighAck = useCallback((seq: number) => {
    setAckedHighSeqs((prev) => {
      const set = new Set(prev);
      if (set.has(seq)) set.delete(seq);
      else set.add(seq);
      return Array.from(set).sort((a, b) => a - b);
    });
  }, []);

  const toggleMediumAck = useCallback((key: string) => {
    setAckedMediumGroupKeys((prev) => {
      const set = new Set(prev);
      if (set.has(key)) set.delete(key);
      else set.add(key);
      return Array.from(set).sort();
    });
  }, []);

  // Phase C: SSE-driven apply orchestrator hook.
  const apply = useCleanupApply(userId);

  const handleApply = useCallback(
    (riskMax: RiskMax) => {
      // Phase D: emit telemetry before kicking off the apply (best-effort).
      telemetry.emitNow();
      void apply.startApply(planId, riskMax, {
        acknowledgedHighRiskSeqs: ackedHighSeqs,
        acknowledgedMediumGroups: ackedMediumGroupKeys,
      });
    },
    [apply, planId, ackedHighSeqs, ackedMediumGroupKeys, telemetry],
  );

  const accountAggregates = useMemo(() => {
    const map = new Map<string, AccountAggregate>();
    for (const op of opsResult.items) {
      let agg = map.get(op.accountId);
      if (!agg) {
        agg = {
          accountId: op.accountId,
          counts: {},
          risk: { low: 0, medium: 0, high: 0 },
          groups: new Map(),
        };
        map.set(op.accountId, agg);
      }
      agg.risk[op.risk] += 1;
      const actionKey = op.action.type as keyof AccountAggregate['counts'];
      agg.counts[actionKey] = (agg.counts[actionKey] ?? 0) + 1;

      const sKey = sourceKey(op.source);
      let g = agg.groups.get(sKey);
      if (!g) {
        g = {
          sourceKey: sKey,
          sourceLabel: sourceLabel(op.source),
          rows: [],
          risk: 'low',
        };
        agg.groups.set(sKey, g);
      }
      g.rows.push(op);
    }
    // Compute highest risk per group
    for (const agg of map.values()) {
      for (const g of agg.groups.values()) {
        g.risk = highestRisk(g.rows);
      }
    }
    return map;
  }, [opsResult.items]);

  if (planQuery.isLoading) {
    return (
      <div className="flex items-center justify-center py-16" role="status" aria-live="polite">
        <div className="flex flex-col items-center gap-3">
          <div className="w-8 h-8 border-4 border-blue-500 border-t-transparent rounded-full animate-spin" />
          <p className="text-sm text-gray-500 dark:text-gray-400">Loading plan…</p>
        </div>
      </div>
    );
  }

  if (planQuery.isError || !planQuery.data) {
    return (
      <div className="rounded-lg border border-red-300 dark:border-red-700 bg-red-50 dark:bg-red-900/20 p-6">
        <h2 className="text-base font-semibold text-red-900 dark:text-red-200">
          Unable to load plan
        </h2>
        <p className="mt-1 text-sm text-red-700 dark:text-red-300">
          {planQuery.error instanceof Error ? planQuery.error.message : 'Unknown error.'}
        </p>
        <button
          type="button"
          onClick={onCancel}
          className="mt-3 px-4 py-2 text-sm font-medium text-white bg-red-600 rounded-md hover:bg-red-700"
        >
          Back to wizard
        </button>
      </div>
    );
  }

  const plan = planQuery.data;

  // Phase C: once an apply job exists, swap the review surface for the
  // SSE-driven progress view. Going `idle` again (e.g. after `onClose`) is
  // not currently supported — `useCleanupApply` keeps the terminal state
  // until unmount.
  if (apply.jobId !== null && apply.jobState !== 'idle') {
    return (
      <div className="space-y-4">
        <header>
          <h1 className="text-lg font-bold text-gray-900 dark:text-gray-100">
            Applying cleanup plan
          </h1>
          <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
            Plan {plan.id} · job {apply.jobId}
          </p>
        </header>
        <CleanupProgress
          jobState={apply.jobState}
          counts={apply.counts}
          perAction={apply.perAction}
          accountStates={apply.accountStates}
          errorMessage={apply.error}
          onCancel={() => void apply.cancelApply()}
          onClose={onCancel}
        />
      </div>
    );
  }

  const isTerminal = plan.status === 'expired' || plan.status === 'cancelled';

  if (isTerminal) {
    return (
      <div className="rounded-lg border border-gray-300 dark:border-gray-700 bg-gray-50 dark:bg-gray-800 p-6 space-y-3">
        <h2 className="text-base font-semibold text-gray-900 dark:text-gray-100">
          Plan {plan.status}
        </h2>
        <p className="text-sm text-gray-600 dark:text-gray-300">
          This plan is no longer actionable. Build a new plan to continue cleaning your inbox.
        </p>
        <button
          type="button"
          onClick={onCancel}
          className="px-4 py-2 text-sm font-medium text-white bg-indigo-600 rounded-md hover:bg-indigo-700"
        >
          Build a new plan
        </button>
      </div>
    );
  }

  // Stale-age check (>25min since plan.createdAt) flags accounts that haven't been refreshed yet.
  const createdAtMs = new Date(plan.createdAt).getTime();
  const isStale = Number.isFinite(createdAtMs) && Date.now() - createdAtMs > STALE_MS;
  const staleAccounts = isStale ? plan.accountIds.filter((id) => !refreshedAccounts.has(id)) : [];

  const totalOps = plan.totals.totalOperations;
  const accountCount = plan.accountIds.length;

  return (
    <div className="space-y-6" aria-labelledby="cleanup-review-heading">
      {readOnly && (
        <div
          role="status"
          aria-live="polite"
          className="rounded-md border border-gray-300 dark:border-gray-700 bg-gray-50 dark:bg-gray-800 px-3 py-2 text-xs text-gray-700 dark:text-gray-300"
        >
          Read-only — viewing past plan. Apply, refresh, and acknowledgment controls are disabled.
        </div>
      )}
      {/* 1. Top-level summary */}
      <header className="rounded-lg border border-indigo-200 dark:border-indigo-800 bg-indigo-50 dark:bg-indigo-900/20 p-4">
        <h1
          id="cleanup-review-heading"
          className="text-lg font-bold text-indigo-900 dark:text-indigo-100"
        >
          Review your cleanup plan
        </h1>
        <p className="mt-1 text-sm text-indigo-800 dark:text-indigo-200">
          <span className="font-semibold">{totalOps.toLocaleString()}</span> operation
          {totalOps === 1 ? '' : 's'} across <span className="font-semibold">{accountCount}</span>{' '}
          account
          {accountCount === 1 ? '' : 's'}
          {' · '}
          <span className="font-semibold">{plan.risk.low}</span> low,{' '}
          <span className="font-semibold">{plan.risk.medium}</span> medium,{' '}
          <span className="font-semibold">{plan.risk.high}</span> high risk
        </p>
        <p className="mt-1 text-xs text-indigo-700 dark:text-indigo-300">
          Plan {plan.id} · valid until {new Date(plan.validUntil).toLocaleString()} · status{' '}
          <span className="font-medium">{plan.status}</span>
        </p>
      </header>

      {/* 2. Risk legend */}
      <RiskLegend />

      {/* 3. Warnings */}
      {plan.warnings.length > 0 && <PlanWarningsBanner warnings={plan.warnings} />}

      {/* 4. Stale-age refresh affordances */}
      {!readOnly && staleAccounts.length > 0 && (
        <div className="space-y-2">
          {staleAccounts.map((accountId) => (
            <RefreshAccountAffordance
              key={accountId}
              planId={planId}
              userId={userId}
              accountId={accountId}
              reason="staleAge"
              onRefreshed={() => {
                setRefreshedAccounts((prev) => {
                  const next = new Set(prev);
                  next.add(accountId);
                  return next;
                });
                void planQuery.refetch();
                opsResult.refetch();
              }}
            />
          ))}
        </div>
      )}

      {/* 5. Account summary tiles */}
      {opsResult.isLoading && opsResult.items.length === 0 ? (
        <div className="text-sm text-gray-500 dark:text-gray-400" role="status" aria-live="polite">
          Loading operations…
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          {Array.from(accountAggregates.values()).map((agg) => (
            <AccountSummaryTile
              key={agg.accountId}
              accountId={agg.accountId}
              accountLabel={agg.accountId}
              provider={detectProvider(plan, agg.accountId)}
              counts={agg.counts}
              risk={agg.risk}
            />
          ))}
        </div>
      )}

      {/* 6. Diff groups per (account, source) */}
      <div className="space-y-4">
        {Array.from(accountAggregates.values()).map((agg) => (
          <section
            key={agg.accountId}
            aria-labelledby={`diff-account-${agg.accountId}-heading`}
            className="space-y-2"
          >
            <h2
              id={`diff-account-${agg.accountId}-heading`}
              className="text-sm font-semibold text-gray-700 dark:text-gray-300"
            >
              {agg.accountId}
            </h2>
            {Array.from(agg.groups.values()).map((group) => (
              <PlanDiffGroup
                key={`${agg.accountId}:${group.sourceKey}`}
                accountId={agg.accountId}
                group={group}
                planId={planId}
                userId={userId}
                ackedHighSeqs={ackedHighSet}
                ackedMediumGroupKeys={ackedMediumSet}
                onToggleHighAck={toggleHighAck}
                onToggleMediumAck={toggleMediumAck}
                onGroupExpanded={telemetry.noteGroupExpanded}
                onSampleViewed={telemetry.noteSampleViewed}
                readOnly={readOnly}
              />
            ))}
          </section>
        ))}
      </div>

      {opsResult.hasNextPage && (
        <div className="text-center">
          <button
            type="button"
            onClick={opsResult.fetchNextPage}
            disabled={opsResult.isFetchingNextPage}
            className="px-4 py-2 text-sm font-medium text-blue-700 dark:text-blue-300 border border-blue-300 dark:border-blue-700 rounded-md hover:bg-blue-50 dark:hover:bg-blue-900/20 disabled:opacity-50"
          >
            {opsResult.isFetchingNextPage ? 'Loading…' : 'Load more operations'}
          </button>
        </div>
      )}

      {/* 7. Risk acknowledger */}
      {!readOnly && <RiskAcknowledger rows={opsResult.items} onAcksChange={handleAcksChange} />}

      {/* 8. Apply buttons */}
      {!readOnly && (
        <ApplyButtons
          planId={planId}
          userId={userId}
          rows={opsResult.items}
          ackedHighSeqs={ackedHighSeqs}
          ackedMediumGroupKeys={ackedMediumGroupKeys}
          onApply={handleApply}
          accountProviders={plan.accountProviders}
          accountStateEtags={plan.accountStateEtags}
        />
      )}

      <div className="flex justify-end pt-2 border-t border-gray-200 dark:border-gray-700">
        <button
          type="button"
          onClick={onCancel}
          className="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 border border-gray-300 dark:border-gray-600 rounded-md hover:bg-gray-50 dark:hover:bg-gray-700"
        >
          {readOnly ? 'Back to history' : 'Back to wizard'}
        </button>
      </div>
    </div>
  );
}
