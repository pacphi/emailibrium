export type ProgressBarStatus = 'pending' | 'running' | 'complete' | 'error';

interface ProgressBarProps {
  value: number;
  label: string;
  status: ProgressBarStatus;
  showPercentage?: boolean;
}

const statusColors: Record<ProgressBarStatus, string> = {
  pending: 'bg-gray-300 dark:bg-gray-600',
  running: 'bg-blue-500',
  complete: 'bg-green-500',
  error: 'bg-red-500',
};

const statusTrackColors: Record<ProgressBarStatus, string> = {
  pending: 'bg-gray-200 dark:bg-gray-700',
  running: 'bg-blue-100 dark:bg-blue-900/30',
  complete: 'bg-green-100 dark:bg-green-900/30',
  error: 'bg-red-100 dark:bg-red-900/30',
};

export function ProgressBar({ value, label, status, showPercentage = true }: ProgressBarProps) {
  const clampedValue = Math.min(100, Math.max(0, value));

  return (
    <div className="w-full">
      <div className="flex items-center justify-between mb-1">
        <span className="text-sm font-medium text-gray-700 dark:text-gray-300">{label}</span>
        {showPercentage && (
          <span className="text-sm text-gray-500 dark:text-gray-400">
            {Math.round(clampedValue)}%
          </span>
        )}
      </div>
      <div
        className={`w-full h-2.5 rounded-full overflow-hidden ${statusTrackColors[status]}`}
        role="progressbar"
        aria-valuenow={clampedValue}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label={label}
      >
        <div
          className={`h-full rounded-full transition-all duration-500 ease-out ${statusColors[status]} ${
            status === 'running' ? 'animate-pulse' : ''
          }`}
          style={{ width: `${clampedValue}%` }}
        />
      </div>
    </div>
  );
}
