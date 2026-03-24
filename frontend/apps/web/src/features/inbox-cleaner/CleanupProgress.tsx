import { ProgressBar } from './ProgressBar';
import type { ProgressBarStatus } from './ProgressBar';

export type CleanupActionType = 'unsubscribe' | 'archive' | 'delete';

export interface CleanupAction {
  type: CleanupActionType;
  total: number;
  completed: number;
  failed: number;
}

export type CleanupState = 'running' | 'done' | 'error';

interface CleanupProgressProps {
  actions: CleanupAction[];
  state: CleanupState;
  onDone?: () => void;
  errors: string[];
}

const actionLabels: Record<CleanupActionType, string> = {
  unsubscribe: 'Unsubscribing',
  archive: 'Archiving',
  delete: 'Deleting',
};

const actionIcons: Record<CleanupActionType, string> = {
  unsubscribe:
    'M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728A9 9 0 015.636 5.636m12.728 12.728L5.636 5.636',
  archive: 'M5 8h14M5 8a2 2 0 110-4h14a2 2 0 110 4M5 8v10a2 2 0 002 2h10a2 2 0 002-2V8m-9 4h4',
  delete:
    'M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16',
};

function getActionStatus(action: CleanupAction, state: CleanupState): ProgressBarStatus {
  if (action.failed > 0 && action.completed + action.failed >= action.total) return 'error';
  if (action.completed >= action.total) return 'complete';
  if (action.completed > 0 || state === 'running') return 'running';
  return 'pending';
}

export function CleanupProgress({ actions, state, onDone, errors }: CleanupProgressProps) {
  const totalCompleted = actions.reduce((sum, a) => sum + a.completed, 0);
  const totalItems = actions.reduce((sum, a) => sum + a.total, 0);
  const totalFailed = actions.reduce((sum, a) => sum + a.failed, 0);

  return (
    <div className="space-y-6">
      {/* Overall progress */}
      <div className="text-center">
        {state === 'running' && (
          <div className="flex flex-col items-center gap-3 mb-4">
            <div className="w-10 h-10 border-4 border-blue-500 border-t-transparent rounded-full animate-spin" />
            <p className="text-sm font-medium text-gray-700 dark:text-gray-300">
              Cleaning up your inbox...
            </p>
          </div>
        )}
        {state === 'done' && (
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
              Cleanup Complete
            </p>
            <p className="text-xs text-gray-500 dark:text-gray-400">
              Successfully processed {totalCompleted.toLocaleString()} of{' '}
              {totalItems.toLocaleString()} items
              {totalFailed > 0 && ` (${totalFailed} failed)`}
            </p>
          </div>
        )}
        {state === 'error' && (
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
          </div>
        )}
      </div>

      {/* Per-action progress */}
      <div className="space-y-4 rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4">
        {actions.map((action) => {
          const status = getActionStatus(action, state);
          const progress =
            action.total > 0 ? Math.round((action.completed / action.total) * 100) : 0;

          return (
            <div key={action.type} className="space-y-1">
              <div className="flex items-center gap-2">
                <svg
                  className="w-4 h-4 text-gray-400 shrink-0"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d={actionIcons[action.type]}
                  />
                </svg>
                <span className="text-xs text-gray-500 dark:text-gray-400">
                  {action.completed.toLocaleString()} / {action.total.toLocaleString()}
                  {action.failed > 0 && (
                    <span className="text-red-500 ml-1">({action.failed} failed)</span>
                  )}
                </span>
              </div>
              <ProgressBar label={actionLabels[action.type]} value={progress} status={status} />
            </div>
          );
        })}
      </div>

      {/* Errors */}
      {errors.length > 0 && (
        <div className="rounded-lg border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/20 p-4">
          <h4 className="text-sm font-semibold text-red-700 dark:text-red-400 mb-2">Errors</h4>
          <ul className="space-y-1">
            {errors.map((err, i) => (
              <li key={i} className="text-xs text-red-600 dark:text-red-300">
                {err}
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Done button */}
      {state === 'done' && onDone && (
        <div className="flex justify-center pt-2">
          <button
            onClick={onDone}
            className="px-6 py-2.5 text-sm font-medium text-white bg-indigo-600 rounded-md hover:bg-indigo-700 transition-colors"
          >
            Done
          </button>
        </div>
      )}
    </div>
  );
}
