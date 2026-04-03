import { useQuery } from '@tanstack/react-query';
import { getIngestionProgress } from '@emailibrium/api';
import { useSyncStore } from '@/shared/stores/syncStore';
import { useSyncStatus } from './hooks/useSyncStatus';

/**
 * Compact sync status indicator showing pipeline state, pending offline
 * operations, and online/offline state. Placed in the Command Center header.
 */
export function SyncStatusIndicator() {
  const { pendingCount, isOnline, flush } = useSyncStatus();
  const syncing = useSyncStore((s) => s.syncing);

  // Check if the ingestion pipeline is active (independent of Zustand sync state).
  const ingestionQuery = useQuery({
    queryKey: ['dashboard-ingestion-progress'],
    queryFn: getIngestionProgress,
    staleTime: 2000,
    refetchInterval: 3000,
  });
  const pipelineActive =
    ingestionQuery.data?.active === true && ingestionQuery.data?.phase !== 'complete';

  // Pipeline or sync in progress → show "Processing" indicator
  if (pipelineActive || syncing) {
    return (
      <div className="flex items-center gap-1.5 text-xs text-indigo-600 dark:text-indigo-400">
        <span className="h-2 w-2 animate-pulse rounded-full bg-indigo-500" aria-hidden="true" />
        Processing
      </div>
    );
  }

  // Offline
  if (!isOnline) {
    return (
      <div className="flex items-center gap-2">
        <div className="flex items-center gap-1.5 text-xs text-red-600 dark:text-red-400">
          <span className="h-2 w-2 animate-pulse rounded-full bg-red-500" aria-hidden="true" />
          Offline
        </div>
        {pendingCount > 0 && (
          <button
            type="button"
            onClick={flush}
            disabled
            className="flex items-center gap-1 rounded-full border border-amber-200 bg-amber-50 px-2 py-0.5 text-xs font-medium text-amber-700 opacity-50 dark:border-amber-800 dark:bg-amber-900/20 dark:text-amber-400"
            title="Waiting for connection"
          >
            <SyncIcon />
            {pendingCount} pending
          </button>
        )}
      </div>
    );
  }

  // Pending offline operations
  if (pendingCount > 0) {
    return (
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={flush}
          className="flex items-center gap-1 rounded-full border border-amber-200 bg-amber-50 px-2 py-0.5 text-xs font-medium text-amber-700 transition-colors hover:bg-amber-100 dark:border-amber-800 dark:bg-amber-900/20 dark:text-amber-400 dark:hover:bg-amber-900/30"
          title="Click to sync pending operations"
        >
          <SyncIcon />
          {pendingCount} pending
        </button>
      </div>
    );
  }

  // All clear
  return (
    <div className="flex items-center gap-1.5 text-xs text-green-600 dark:text-green-400">
      <span className="h-2 w-2 rounded-full bg-green-500" aria-hidden="true" />
      Synced
    </div>
  );
}

function SyncIcon() {
  return (
    <svg
      className="h-3 w-3"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
      />
    </svg>
  );
}
