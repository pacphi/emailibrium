import { useState } from 'react';
import type { PlanId } from '@emailibrium/types';
import { refreshPlanAccount } from '@emailibrium/api';

export interface RefreshAccountAffordanceProps {
  planId: PlanId;
  userId: string;
  accountId: string;
  reason: 'hardDrift' | 'staleAge';
  onRefreshed(): void;
}

const reasonCopy: Record<RefreshAccountAffordanceProps['reason'], { title: string; body: string }> =
  {
    hardDrift: {
      title: 'Account state has drifted',
      body: 'The mailbox state for this account changed since the plan was built. Refresh to ensure operations target the right messages.',
    },
    staleAge: {
      title: 'Plan is getting stale',
      body: 'This plan was built more than 25 minutes ago. Refreshing will re-resolve predicates against the latest mailbox state.',
    },
  };

export function RefreshAccountAffordance({
  planId,
  userId,
  accountId,
  reason,
  onRefreshed,
}: RefreshAccountAffordanceProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const copy = reasonCopy[reason];

  const handleClick = async () => {
    setBusy(true);
    setError(null);
    try {
      await refreshPlanAccount(planId, userId, accountId);
      onRefreshed();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      role="alert"
      className="rounded-lg border border-blue-300 dark:border-blue-700 bg-blue-50 dark:bg-blue-900/20 px-4 py-3 flex items-start gap-3"
    >
      <span aria-hidden="true" className="text-blue-700 dark:text-blue-300 font-bold">
        ↻
      </span>
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium text-blue-900 dark:text-blue-200">{copy.title}</p>
        <p className="mt-0.5 text-xs text-blue-700 dark:text-blue-300">
          {copy.body} <span className="font-mono">({accountId})</span>
        </p>
        {error && (
          <p className="mt-1 text-xs text-red-600 dark:text-red-400" role="alert">
            Refresh failed: {error}
          </p>
        )}
      </div>
      <button
        type="button"
        onClick={handleClick}
        disabled={busy}
        aria-disabled={busy}
        className="shrink-0 px-3 py-1.5 text-xs font-medium text-white bg-blue-600 rounded-md hover:bg-blue-700 disabled:opacity-50 disabled:cursor-wait"
      >
        {busy ? 'Refreshing…' : 'Refresh this account'}
      </button>
    </div>
  );
}
