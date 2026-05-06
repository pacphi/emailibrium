import { useMemo, useState } from 'react';
import type {
  AccountStateEtag,
  CleanupProvider,
  PlanId,
  PlannedOperation,
  RiskMax,
} from '@emailibrium/types';
import { opMediumGroupKey } from './groupKey';

export interface ApplyButtonsProps {
  planId: PlanId;
  userId: string;
  rows: PlannedOperation[];
  ackedHighSeqs: number[];
  ackedMediumGroupKeys: string[];
  onApply(riskMax: RiskMax): void;
  /**
   * Phase D: per-account provider lookup from the plan envelope. Authoritative
   * source for POP3 detection — the High-tier "Apply all" button gains a typed
   * "DELETE" confirmation when any account is POP3.
   */
  accountProviders?: Record<string, CleanupProvider>;
  /**
   * Phase D: per-account state etags. Retained as a fallback when older cached
   * plans don't carry `accountProviders` (we then infer POP3 from etag
   * `kind === 'none'`, the only shape POP3 used pre-Phase-D).
   */
  accountStateEtags?: Record<string, AccountStateEtag>;
}

interface ButtonState {
  enabled: boolean;
  reason?: string;
}

export function ApplyButtons({
  planId,
  userId,
  rows,
  ackedHighSeqs,
  ackedMediumGroupKeys,
  onApply,
  accountProviders,
  accountStateEtags,
}: ApplyButtonsProps) {
  // Phase D: typed-confirmation gate for POP3 accounts (ADR-030 §Security).
  // Closes the prior TODO that inferred POP3 from `etag.kind === 'none'` —
  // we now consult `plan.accountProviders` directly and only fall back to
  // the etag-kind heuristic for older cached plans without the new field.
  const hasPop3Account = useMemo(() => {
    if (accountProviders && Object.keys(accountProviders).length > 0) {
      return Object.values(accountProviders).some((p) => p === 'pop3');
    }
    if (!accountStateEtags) return false;
    return Object.values(accountStateEtags).some((e) => e.kind === 'none');
  }, [accountProviders, accountStateEtags]);
  const [typedConfirmation, setTypedConfirmation] = useState('');
  const typedConfirmationOk = !hasPop3Account || typedConfirmation === 'DELETE';
  const ackedHighSet = useMemo(() => new Set(ackedHighSeqs), [ackedHighSeqs]);
  const ackedMediumSet = useMemo(() => new Set(ackedMediumGroupKeys), [ackedMediumGroupKeys]);

  const { lowState, mediumState, highState, lowCount, mediumCount, highCount, totalMediumGroups } =
    useMemo(() => {
      let low = 0;
      let medium = 0;
      let high = 0;
      const allMediumGroups = new Set<string>();
      const unackedHigh: number[] = [];
      const unackedMediumGroups: string[] = [];

      for (const op of rows) {
        if (op.risk === 'low') low += 1;
        else if (op.risk === 'medium') {
          medium += 1;
          const key = opMediumGroupKey(op);
          allMediumGroups.add(key);
          if (!ackedMediumSet.has(key)) unackedMediumGroups.push(key);
        } else if (op.risk === 'high') {
          high += 1;
          if (!ackedHighSet.has(op.seq)) unackedHigh.push(op.seq);
          // High rows also need their medium-group acked (the group key is
          // computed identically); track here too.
          const key = opMediumGroupKey(op);
          allMediumGroups.add(key);
          if (!ackedMediumSet.has(key)) unackedMediumGroups.push(key);
        }
      }

      const lowS: ButtonState = {
        enabled: low > 0,
        reason: low === 0 ? 'No low-risk operations in plan' : undefined,
      };

      const mediumS: ButtonState = {
        enabled: medium > 0 && unackedMediumGroups.length === 0,
        reason:
          medium === 0
            ? 'No medium-risk operations in plan'
            : unackedMediumGroups.length > 0
              ? `${unackedMediumGroups.length} medium group${
                  unackedMediumGroups.length === 1 ? '' : 's'
                } still need acknowledgment`
              : undefined,
      };

      const highS: ButtonState = {
        enabled:
          unackedHigh.length === 0 && unackedMediumGroups.length === 0 && typedConfirmationOk,
        reason:
          unackedHigh.length > 0
            ? `${unackedHigh.length} high-risk row${
                unackedHigh.length === 1 ? '' : 's'
              } still need acknowledgment`
            : unackedMediumGroups.length > 0
              ? `${unackedMediumGroups.length} medium group${
                  unackedMediumGroups.length === 1 ? '' : 's'
                } still need acknowledgment`
              : !typedConfirmationOk
                ? 'Type DELETE to confirm — POP3 deletes are irreversible'
                : undefined,
      };

      return {
        lowState: lowS,
        mediumState: mediumS,
        highState: highS,
        lowCount: low,
        mediumCount: medium,
        highCount: high,
        totalMediumGroups: allMediumGroups.size,
      };
    }, [rows, ackedHighSet, ackedMediumSet, typedConfirmationOk]);

  // Phase C: parent (CleanupReview) holds the useCleanupApply hook and
  // turns this callback into a real POST /apply. planId/userId are kept on
  // the props for future telemetry / logging hooks.
  void planId;
  void userId;
  const handleApply = (riskMax: RiskMax) => {
    onApply(riskMax);
  };

  const renderButton = (label: string, riskMax: RiskMax, state: ButtonState, classes: string) => (
    <button
      type="button"
      onClick={() => state.enabled && handleApply(riskMax)}
      disabled={!state.enabled}
      aria-disabled={!state.enabled}
      title={state.reason}
      className={`px-4 py-2 text-sm font-medium rounded-md transition-colors ${classes} ${
        state.enabled ? '' : 'opacity-50 cursor-not-allowed'
      }`}
    >
      {label}
    </button>
  );

  return (
    <section aria-labelledby="apply-heading" className="space-y-3">
      <h2 id="apply-heading" className="text-sm font-semibold text-gray-900 dark:text-gray-100">
        Apply plan
      </h2>
      <p className="text-xs text-gray-500 dark:text-gray-400">
        Acknowledged: {ackedMediumSet.size}/{totalMediumGroups} medium groups, {ackedHighSet.size}/
        {highCount} high-risk rows
      </p>
      <div className="flex flex-wrap gap-3">
        {renderButton(
          `Apply Low only (${lowCount.toLocaleString()})`,
          'low',
          lowState,
          'bg-green-600 text-white hover:bg-green-700',
        )}
        {renderButton(
          `Apply Low + Medium (${(lowCount + mediumCount).toLocaleString()})`,
          'medium',
          mediumState,
          'bg-amber-600 text-white hover:bg-amber-700',
        )}
        {renderButton(
          `Apply all incl. High (${(lowCount + mediumCount + highCount).toLocaleString()})`,
          'high',
          highState,
          'bg-red-600 text-white hover:bg-red-700',
        )}
      </div>
      {hasPop3Account && (
        <div className="rounded-md border border-red-300 dark:border-red-700 bg-red-50 dark:bg-red-900/20 px-3 py-2">
          <label
            htmlFor="pop3-typed-confirm"
            className="block text-xs font-medium text-red-900 dark:text-red-200"
          >
            POP3 deletes are irreversible. Type <span className="font-mono font-bold">DELETE</span>{' '}
            to enable Apply all.
          </label>
          <input
            id="pop3-typed-confirm"
            type="text"
            value={typedConfirmation}
            onChange={(e) => setTypedConfirmation(e.target.value)}
            autoComplete="off"
            spellCheck={false}
            placeholder="DELETE"
            aria-label="Type DELETE to enable Apply all on POP3 account"
            className="mt-1.5 w-32 px-2 py-1 text-xs font-mono border border-red-300 dark:border-red-600 rounded bg-white dark:bg-gray-800 text-red-900 dark:text-red-100 focus:outline-none focus:ring-2 focus:ring-red-500"
          />
          {typedConfirmationOk && (
            <p
              className="mt-1 text-[11px] text-red-700 dark:text-red-300"
              role="status"
              aria-live="polite"
            >
              Confirmation accepted.
            </p>
          )}
        </div>
      )}
    </section>
  );
}
