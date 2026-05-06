// Phase C: SSE-driven apply progress view. Replaces the Phase B/A simulator
// shape (`CleanupAction[]` / `CleanupState`) with the orchestrator wire
// types. The legacy `CleanupAction` / `CleanupState` exports are gone; the
// only call site (InboxCleaner) was the simulator and that has been removed.

import type { AccountSnapshotState, ApplyJobCounts, PauseReason } from '@emailibrium/types';
import { ProgressBar } from './ProgressBar';
import type { ProgressBarStatus } from './ProgressBar';

export type ApplyJobUiState = 'idle' | 'starting' | 'running' | 'done' | 'error' | 'cancelled';

export interface PerActionCounts {
  applied: number;
  failed: number;
  skipped: number;
  /**
   * Phase D: per-action `pending` is no longer authoritatively known once the
   * SSE stream drives breakdowns directly. Optional; the progress UI treats
   * absent `pending` as 0 (i.e. "all observed are processed").
   */
  pending?: number;
}

export interface CleanupProgressProps {
  jobState: ApplyJobUiState;
  counts: ApplyJobCounts;
  /** Optional per-action breakdown (computed by the parent from plan.operations). */
  perAction?: Record<string, PerActionCounts>;
  /** Per-account paused/active state — surfaced in account chips. */
  accountStates?: Record<string, AccountSnapshotState>;
  errorMessage?: string | null;
  onCancel?(): void;
  onClose?(): void;
}

const pauseReasonLabel: Record<PauseReason, string> = {
  hardDrift: 'state drifted',
  rateLimit: 'rate-limited',
  authError: 'auth error',
};

const actionLabels: Record<string, string> = {
  archive: 'Archiving',
  addLabel: 'Labeling',
  move: 'Moving',
  delete: 'Deleting',
  unsubscribe: 'Unsubscribing',
  markRead: 'Marking read',
  star: 'Starring',
};

function totalProcessed(c: ApplyJobCounts): number {
  return c.applied + c.failed + c.skipped;
}

function totalAll(c: ApplyJobCounts): number {
  return c.applied + c.failed + c.skipped + c.pending;
}

function actionStatus(c: PerActionCounts, jobState: ApplyJobUiState): ProgressBarStatus {
  const pending = c.pending ?? 0;
  const total = c.applied + c.failed + c.skipped + pending;
  if (total === 0) return 'pending';
  if (c.failed > 0 && pending === 0) return 'error';
  if (pending === 0) return 'complete';
  if (jobState === 'running' || jobState === 'starting') return 'running';
  return 'pending';
}

export function CleanupProgress({
  jobState,
  counts,
  perAction,
  accountStates,
  errorMessage,
  onCancel,
  onClose,
}: CleanupProgressProps) {
  const total = totalAll(counts);
  const processed = totalProcessed(counts);
  const overallPct = total > 0 ? Math.round((processed / total) * 100) : 0;

  const isRunning = jobState === 'running' || jobState === 'starting';
  const isDone = jobState === 'done';
  const isError = jobState === 'error';
  const isCancelled = jobState === 'cancelled';

  const accountEntries = accountStates ? Object.entries(accountStates) : [];

  return (
    <div className="space-y-6">
      {/* Overall status */}
      <div className="text-center">
        {isRunning && (
          <div className="flex flex-col items-center gap-3 mb-4">
            <div className="w-10 h-10 border-4 border-blue-500 border-t-transparent rounded-full animate-spin" />
            <p className="text-sm font-medium text-gray-700 dark:text-gray-300">
              {jobState === 'starting' ? 'Starting cleanup…' : 'Cleaning up your inbox…'}
            </p>
          </div>
        )}
        {isDone && (
          <div className="flex flex-col items-center gap-3 mb-4">
            <div className="w-12 h-12 rounded-full bg-green-100 dark:bg-green-900/30 flex items-center justify-center">
              <svg
                className="w-6 h-6 text-green-600 dark:text-green-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M5 13l4 4L19 7"
                />
              </svg>
            </div>
            <p className="text-sm font-semibold text-green-700 dark:text-green-400">
              Cleanup complete
            </p>
            <p className="text-xs text-gray-500 dark:text-gray-400">
              Applied {counts.applied.toLocaleString()} ·{' '}
              {counts.skipped > 0 && `${counts.skipped.toLocaleString()} skipped · `}
              {counts.failed > 0 && `${counts.failed.toLocaleString()} failed`}
            </p>
          </div>
        )}
        {isCancelled && (
          <div className="flex flex-col items-center gap-3 mb-4">
            <div className="w-12 h-12 rounded-full bg-amber-100 dark:bg-amber-900/30 flex items-center justify-center">
              <svg
                className="w-6 h-6 text-amber-600 dark:text-amber-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M10 9v6m4-6v6M5 7h14l-1 12a2 2 0 01-2 2H8a2 2 0 01-2-2L5 7z"
                />
              </svg>
            </div>
            <p className="text-sm font-semibold text-amber-700 dark:text-amber-400">
              Cleanup cancelled
            </p>
            <p className="text-xs text-gray-500 dark:text-gray-400">
              {counts.applied.toLocaleString()} operations completed before cancel.
            </p>
          </div>
        )}
        {isError && (
          <div className="flex flex-col items-center gap-3 mb-4">
            <div className="w-12 h-12 rounded-full bg-red-100 dark:bg-red-900/30 flex items-center justify-center">
              <svg
                className="w-6 h-6 text-red-600 dark:text-red-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M6 18L18 6M6 6l12 12"
                />
              </svg>
            </div>
            <p className="text-sm font-semibold text-red-700 dark:text-red-400">
              Cleanup encountered errors
            </p>
            {errorMessage && (
              <p className="text-xs text-red-600 dark:text-red-300 max-w-sm">{errorMessage}</p>
            )}
          </div>
        )}
      </div>

      {/* Overall progress bar */}
      <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4 space-y-3">
        <div className="flex items-center justify-between text-xs text-gray-500 dark:text-gray-400">
          <span>
            {processed.toLocaleString()} / {total.toLocaleString()} operations
          </span>
          <span>
            {counts.applied} applied · {counts.skipped} skipped · {counts.failed} failed
          </span>
        </div>
        <ProgressBar
          label="Overall progress"
          value={overallPct}
          status={isError ? 'error' : isDone ? 'complete' : isRunning ? 'running' : 'pending'}
        />

        {/* Per-action mini bars (optional). */}
        {perAction && Object.keys(perAction).length > 0 && (
          <div className="space-y-2 pt-2 border-t border-gray-100 dark:border-gray-700">
            {Object.entries(perAction).map(([actionType, c]) => {
              const pending = c.pending ?? 0;
              const tot = c.applied + c.failed + c.skipped + pending;
              const pct =
                tot > 0 ? Math.round(((c.applied + c.failed + c.skipped) / tot) * 100) : 0;
              return (
                <div key={actionType} className="space-y-1">
                  <div className="flex items-center justify-between text-xs text-gray-500 dark:text-gray-400">
                    <span>{actionLabels[actionType] ?? actionType}</span>
                    <span>
                      {c.applied}/{tot}
                      {c.failed > 0 && (
                        <span className="text-red-500 ml-1">({c.failed} failed)</span>
                      )}
                    </span>
                  </div>
                  <ProgressBar
                    label={actionLabels[actionType] ?? actionType}
                    value={pct}
                    status={actionStatus(c, jobState)}
                  />
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Per-account paused indicators */}
      {accountEntries.some(([, s]) => s.paused) && (
        <div className="rounded-lg border border-amber-300 dark:border-amber-700 bg-amber-50 dark:bg-amber-900/20 p-3 space-y-1">
          <h4 className="text-xs font-semibold text-amber-800 dark:text-amber-300">
            Paused accounts
          </h4>
          <ul className="space-y-1">
            {accountEntries
              .filter(([, s]) => s.paused)
              .map(([accountId, s]) => (
                <li
                  key={accountId}
                  className="text-xs text-amber-700 dark:text-amber-300 flex items-center gap-2"
                >
                  <span className="font-mono">{accountId}</span>
                  <span>
                    paused
                    {s.pauseReason ? ` — ${pauseReasonLabel[s.pauseReason]}` : ''}
                  </span>
                  {s.pauseReason === 'hardDrift' && (
                    <span className="text-[10px] uppercase tracking-wide text-amber-600 dark:text-amber-400">
                      refresh required
                    </span>
                  )}
                </li>
              ))}
          </ul>
        </div>
      )}

      {/* Skipped breakdown */}
      {counts.skippedByReason && Object.keys(counts.skippedByReason).length > 0 && (
        <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/40 p-3">
          <h4 className="text-xs font-semibold text-gray-700 dark:text-gray-300 mb-1">
            Skipped breakdown
          </h4>
          <ul className="space-y-0.5 text-xs text-gray-600 dark:text-gray-400">
            {Object.entries(counts.skippedByReason).map(([reason, n]) => (
              <li key={reason}>
                <span className="font-mono">{reason}</span>: {n}
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Action buttons */}
      <div className="flex justify-center gap-3 pt-2">
        {isRunning && onCancel && (
          <button
            type="button"
            onClick={onCancel}
            className="px-5 py-2 text-sm font-medium text-amber-700 dark:text-amber-300 border border-amber-300 dark:border-amber-700 rounded-md hover:bg-amber-50 dark:hover:bg-amber-900/20 transition-colors"
          >
            Cancel
          </button>
        )}
        {(isDone || isCancelled || isError) && onClose && (
          <button
            type="button"
            onClick={onClose}
            className="px-6 py-2.5 text-sm font-medium text-white bg-indigo-600 rounded-md hover:bg-indigo-700 transition-colors"
          >
            {isDone ? 'View results' : 'Done'}
          </button>
        )}
      </div>
    </div>
  );
}
