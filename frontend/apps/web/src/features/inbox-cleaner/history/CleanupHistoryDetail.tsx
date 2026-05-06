// Phase D — Read-only detail view for a past plan (`/cleanup/history/:planId`).
//
// Reuses the existing CleanupReview by passing `readOnly`. Below the review,
// fetches and displays the apply audit-trail (Phase D backend). When the
// audit endpoint is unavailable (the parallel backend agent has not landed
// yet, or the plan was never applied), gracefully falls back to a small
// notice rather than failing the page.

import { useQuery } from '@tanstack/react-query';
import type { PlanId } from '@emailibrium/types';
import type { CleanupAuditEntry } from '@emailibrium/api';
import { listPlanAudit } from '@emailibrium/api';
import { CleanupReview } from '../review/CleanupReview';

export interface CleanupHistoryDetailProps {
  userId: string | null;
  planId: PlanId;
}

const outcomePillClasses: Record<CleanupAuditEntry['outcome'], string> = {
  applied: 'bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-200',
  failed: 'bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-200',
  skipped: 'bg-gray-100 text-gray-800 dark:bg-gray-700 dark:text-gray-200',
};

function AuditPanel({ userId, planId }: { userId: string; planId: PlanId }) {
  const auditQuery = useQuery({
    queryKey: ['cleanup', 'plan-audit', planId, userId],
    queryFn: () => listPlanAudit(planId, userId),
    enabled: Boolean(userId && planId),
    // Audit history is append-only and immutable for finished plans, so we
    // don't need a short staleTime. Treat any error as "unavailable".
    retry: false,
    staleTime: 5 * 60_000,
  });

  if (auditQuery.isLoading) {
    return (
      <div role="status" aria-live="polite" className="text-xs text-gray-500 dark:text-gray-400">
        Loading audit history…
      </div>
    );
  }

  if (auditQuery.isError) {
    return (
      <p className="text-xs text-gray-500 dark:text-gray-400">
        Audit history unavailable — see operation status column above.
      </p>
    );
  }

  const entries = auditQuery.data?.items ?? [];
  if (entries.length === 0) {
    return (
      <p className="text-xs text-gray-500 dark:text-gray-400">
        No audit entries recorded for this plan.
      </p>
    );
  }

  return (
    <ul role="list" className="space-y-1 max-h-72 overflow-y-auto">
      {entries.map((entry) => (
        <li
          key={`${entry.seq}-${entry.outcome}-${entry.at}`}
          role="listitem"
          className="flex items-center gap-2 px-2 py-1 text-xs text-gray-700 dark:text-gray-300 border-b border-gray-100 dark:border-gray-700 last:border-b-0"
        >
          <span className="font-mono text-[10px] text-gray-400 w-12 shrink-0">#{entry.seq}</span>
          <span
            className={`inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide shrink-0 ${outcomePillClasses[entry.outcome]}`}
          >
            {entry.outcome}
          </span>
          <span className="font-mono text-[10px] text-gray-500 dark:text-gray-400 truncate">
            {entry.accountId}
          </span>
          <span className="text-[10px] text-gray-400 dark:text-gray-500 shrink-0">
            {new Date(entry.at).toLocaleString()}
          </span>
          {entry.error && (
            <span className="text-[10px] text-red-600 dark:text-red-400 truncate">
              {entry.error.code}: {entry.error.message}
            </span>
          )}
          {entry.skipReason && (
            <span className="text-[10px] text-gray-500 dark:text-gray-400">
              ({entry.skipReason})
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}

export function CleanupHistoryDetail({ userId, planId }: CleanupHistoryDetailProps) {
  if (!userId) {
    return (
      <div className="p-6 max-w-5xl mx-auto">
        <div
          role="status"
          className="rounded-md border border-amber-300 dark:border-amber-700 bg-amber-50 dark:bg-amber-900/20 px-3 py-2 text-sm text-amber-800 dark:text-amber-200"
        >
          Sign in to view this plan.
        </div>
      </div>
    );
  }

  return (
    <div className="p-6 max-w-5xl mx-auto space-y-6">
      <CleanupReview
        planId={planId}
        userId={userId}
        readOnly
        onCancel={() => {
          window.location.href = '/cleanup/history';
        }}
      />

      <section
        aria-labelledby="audit-history-heading"
        className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4 space-y-2"
      >
        <header>
          <h2
            id="audit-history-heading"
            className="text-sm font-semibold text-gray-900 dark:text-gray-100"
          >
            Apply history
          </h2>
          <p className="text-xs text-gray-500 dark:text-gray-400">
            Append-only record of operation outcomes for this plan.
          </p>
        </header>
        <AuditPanel userId={userId} planId={planId} />
      </section>
    </div>
  );
}
