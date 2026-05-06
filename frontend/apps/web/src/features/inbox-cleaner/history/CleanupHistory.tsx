// Phase D — Plan history list view (`/cleanup/history`).
//
// Lists the user's last 20 plans via `listPlans(userId, undefined, 20)`. Each
// row links to the read-only review at `/cleanup/history/:planId`. Empty
// state guides the user back to the Inbox Cleaner wizard.

import { useQuery } from '@tanstack/react-query';
import type { CleanupPlanSummary, PlanStatus } from '@emailibrium/types';
import { listPlans } from '@emailibrium/api';

export interface CleanupHistoryProps {
  userId: string | null;
}

const statusPillClasses: Record<PlanStatus, string> = {
  draft: 'bg-gray-100 text-gray-800 dark:bg-gray-700 dark:text-gray-200',
  ready: 'bg-blue-100 text-blue-800 dark:bg-blue-900/40 dark:text-blue-200',
  applying: 'bg-indigo-100 text-indigo-800 dark:bg-indigo-900/40 dark:text-indigo-200',
  applied: 'bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-200',
  partiallyApplied: 'bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-200',
  failed: 'bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-200',
  expired: 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400',
  cancelled: 'bg-gray-200 text-gray-700 dark:bg-gray-700 dark:text-gray-300',
};

const statusLabels: Record<PlanStatus, string> = {
  draft: 'Draft',
  ready: 'Ready',
  applying: 'Applying',
  applied: 'Applied',
  partiallyApplied: 'Partially applied',
  failed: 'Failed',
  expired: 'Expired',
  cancelled: 'Cancelled',
};

function formatCreatedAt(iso: string): string {
  const d = new Date(iso);
  if (!Number.isFinite(d.getTime())) return iso;
  return d.toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function shortId(id: string): string {
  return id.length <= 8 ? id : id.slice(-8);
}

interface RowProps {
  plan: CleanupPlanSummary;
}

function HistoryRow({ plan }: RowProps) {
  const accountCount = Object.keys(plan.totals.byAccount).length;
  return (
    <a
      href={`/cleanup/history/${plan.id}`}
      aria-label={`Plan ${shortId(plan.id)} from ${formatCreatedAt(plan.createdAt)}, status ${statusLabels[plan.status]}`}
      className="flex items-start justify-between gap-4 rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 px-4 py-3 hover:border-indigo-300 dark:hover:border-indigo-700 hover:shadow-sm transition-colors"
    >
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 truncate">
            Plan {shortId(plan.id)} from {formatCreatedAt(plan.createdAt)}
          </h3>
          <span
            className={`inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide shrink-0 ${statusPillClasses[plan.status]}`}
          >
            {statusLabels[plan.status]}
          </span>
        </div>
        <p className="mt-1 text-xs text-gray-600 dark:text-gray-400">
          <span className="font-medium">{plan.totals.totalOperations.toLocaleString()}</span> op
          {plan.totals.totalOperations === 1 ? '' : 's'} ·{' '}
          <span className="font-medium">{accountCount}</span> account
          {accountCount === 1 ? '' : 's'} · Risk:{' '}
          <span className="font-medium">{plan.risk.low}</span> low,{' '}
          <span className="font-medium">{plan.risk.medium}</span> medium,{' '}
          <span className="font-medium">{plan.risk.high}</span> high
        </p>
      </div>
      <span aria-hidden="true" className="text-gray-400 dark:text-gray-500 shrink-0 mt-0.5">
        ›
      </span>
    </a>
  );
}

export function CleanupHistory({ userId }: CleanupHistoryProps) {
  const plansQuery = useQuery({
    queryKey: ['cleanup', 'plans', userId],
    queryFn: () => listPlans(userId!, undefined, 20),
    enabled: Boolean(userId),
    staleTime: 30_000,
  });

  return (
    <div className="p-6 max-w-3xl mx-auto space-y-4">
      <header>
        <h1 className="text-xl font-bold text-gray-900 dark:text-gray-100">Cleanup History</h1>
        <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
          Your last 20 cleanup plans. Click a row to view its details (read-only).
        </p>
      </header>

      {!userId && (
        <div
          role="status"
          className="rounded-md border border-amber-300 dark:border-amber-700 bg-amber-50 dark:bg-amber-900/20 px-3 py-2 text-sm text-amber-800 dark:text-amber-200"
        >
          Sign in to view your cleanup history.
        </div>
      )}

      {userId && plansQuery.isLoading && (
        <div role="status" aria-live="polite" className="text-sm text-gray-500 dark:text-gray-400">
          Loading plans…
        </div>
      )}

      {userId && plansQuery.isError && (
        <div
          role="alert"
          className="rounded-md border border-red-300 dark:border-red-700 bg-red-50 dark:bg-red-900/20 px-3 py-2 text-sm text-red-800 dark:text-red-200"
        >
          Failed to load plans:{' '}
          {plansQuery.error instanceof Error ? plansQuery.error.message : 'Unknown error'}
        </div>
      )}

      {userId && plansQuery.data && plansQuery.data.items.length === 0 && (
        <div className="rounded-lg border border-dashed border-gray-300 dark:border-gray-700 bg-gray-50 dark:bg-gray-800 px-4 py-8 text-center">
          <p className="text-sm text-gray-600 dark:text-gray-400">
            No cleanup plans yet — start a cleanup from the Inbox Cleaner.
          </p>
          <a
            href="/inbox-cleaner"
            className="mt-3 inline-block px-4 py-2 text-sm font-medium text-white bg-indigo-600 rounded-md hover:bg-indigo-700"
          >
            Open Inbox Cleaner
          </a>
        </div>
      )}

      {userId && plansQuery.data && plansQuery.data.items.length > 0 && (
        <ul role="list" className="space-y-2">
          {plansQuery.data.items.map((plan) => (
            <li key={plan.id} role="listitem">
              <HistoryRow plan={plan} />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
