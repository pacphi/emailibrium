import { useCallback } from 'react';
import { useIngestionProgress } from './hooks/useIngestionProgress';
import { PhaseIndicator } from './PhaseIndicator';
import { ProgressBar } from './ProgressBar';
import { DiscoveryFeed } from './DiscoveryFeed';
import type { IngestionPhase } from '@emailibrium/types';
import type { ProgressBarStatus } from './ProgressBar';

interface IngestionProgressScreenProps {
  jobId: string;
  onComplete?: () => void;
  onCancel?: () => void;
}

function phaseToBarStatus(
  targetPhase: IngestionPhase,
  currentPhase: IngestionPhase,
): ProgressBarStatus {
  const order: IngestionPhase[] = [
    'syncing',
    'embedding',
    'categorizing',
    'clustering',
    'analyzing',
    'complete',
  ];
  const targetIdx = order.indexOf(targetPhase);
  const currentIdx = order.indexOf(currentPhase);

  if (targetIdx < currentIdx) return 'complete';
  if (targetIdx === currentIdx && currentPhase !== 'complete') return 'running';
  if (currentPhase === 'complete') return 'complete';
  return 'pending';
}

function phaseProgress(
  targetPhase: IngestionPhase,
  currentPhase: IngestionPhase,
  processed: number,
  total: number,
): number {
  const status = phaseToBarStatus(targetPhase, currentPhase);
  if (status === 'complete') return 100;
  if (status === 'pending') return 0;
  return total > 0 ? Math.round((processed / total) * 100) : 0;
}

function formatEta(seconds: number | null): string {
  if (seconds === null || seconds <= 0) return '--:--';
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return `${m}:${s.toString().padStart(2, '0')}`;
}

function formatThroughput(rate: number): string {
  if (rate < 1) return '< 1 emails/sec';
  return `${Math.round(rate)} emails/sec`;
}

export function IngestionProgressScreen({
  jobId,
  onComplete,
  onCancel,
}: IngestionProgressScreenProps) {
  const { progress, discoveries, connectionStatus, isPaused, pause, resume } =
    useIngestionProgress(jobId);

  const handlePauseResume = useCallback(async () => {
    if (isPaused) {
      await resume();
    } else {
      await pause();
    }
  }, [isPaused, pause, resume]);

  if (connectionStatus === 'connecting' && !progress) {
    return (
      <div className="flex items-center justify-center p-12">
        <div className="flex flex-col items-center gap-4">
          <div className="w-8 h-8 border-4 border-blue-500 border-t-transparent rounded-full animate-spin" />
          <p className="text-sm text-gray-500 dark:text-gray-400">
            Connecting to ingestion stream...
          </p>
        </div>
      </div>
    );
  }

  if (connectionStatus === 'error' && !progress) {
    return (
      <div className="rounded-lg border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/20 p-6 text-center">
        <p className="text-sm text-red-600 dark:text-red-400">
          Failed to connect to the ingestion stream. Please try again.
        </p>
        <button
          onClick={onCancel}
          className="mt-4 px-4 py-2 text-sm font-medium text-red-600 border border-red-300 rounded-md hover:bg-red-100 dark:text-red-400 dark:border-red-700 dark:hover:bg-red-900/30"
        >
          Go Back
        </button>
      </div>
    );
  }

  const currentPhase = progress?.phase ?? 'syncing';
  const processed = progress?.processed ?? 0;
  const total = progress?.total ?? 0;
  const isComplete = currentPhase === 'complete';

  const phaseBars: Array<{ phase: IngestionPhase; label: string }> = [
    { phase: 'syncing', label: 'Syncing Emails' },
    { phase: 'embedding', label: 'Generating Embeddings' },
    { phase: 'categorizing', label: 'Categorizing' },
    { phase: 'clustering', label: 'Clustering Topics' },
    { phase: 'analyzing', label: 'Analyzing Patterns' },
  ];

  return (
    <div className="space-y-6">
      {/* Phase stepper */}
      <PhaseIndicator currentPhase={currentPhase} />

      {/* Stats row */}
      <div className="grid grid-cols-3 gap-4">
        <StatCard label="Throughput" value={formatThroughput(progress?.emailsPerSecond ?? 0)} />
        <StatCard
          label="Processed"
          value={`${processed.toLocaleString()} / ${total.toLocaleString()}`}
        />
        <StatCard
          label="ETA"
          value={isComplete ? 'Done' : formatEta(progress?.etaSeconds ?? null)}
        />
      </div>

      {/* Phase progress bars */}
      <div className="space-y-3 rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4">
        {phaseBars.map(({ phase, label }) => (
          <ProgressBar
            key={phase}
            label={label}
            value={phaseProgress(phase, currentPhase, processed, total)}
            status={phaseToBarStatus(phase, currentPhase)}
          />
        ))}
      </div>

      {/* Failures indicator */}
      {(progress?.failed ?? 0) > 0 && (
        <div className="rounded-md bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800 px-4 py-3">
          <p className="text-sm text-amber-700 dark:text-amber-400">
            {progress!.failed.toLocaleString()} email{progress!.failed > 1 ? 's' : ''} failed to
            process and will be retried.
          </p>
        </div>
      )}

      {/* Discovery feed */}
      <DiscoveryFeed discoveries={discoveries} />

      {/* Action buttons */}
      <div className="flex items-center justify-between pt-2">
        <button
          onClick={onCancel}
          className="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 border border-gray-300 dark:border-gray-600 rounded-md hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
        >
          Cancel
        </button>
        <div className="flex gap-3">
          {!isComplete && (
            <button
              onClick={handlePauseResume}
              className="px-4 py-2 text-sm font-medium text-blue-600 dark:text-blue-400 border border-blue-300 dark:border-blue-700 rounded-md hover:bg-blue-50 dark:hover:bg-blue-900/30 transition-colors"
            >
              {isPaused ? 'Resume' : 'Pause'}
            </button>
          )}
          {isComplete && onComplete && (
            <button
              onClick={onComplete}
              className="px-5 py-2 text-sm font-medium text-white bg-green-600 rounded-md hover:bg-green-700 transition-colors"
            >
              Continue to Cleanup
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function StatCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 px-4 py-3 text-center">
      <p className="text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
        {label}
      </p>
      <p className="mt-1 text-lg font-semibold text-gray-900 dark:text-gray-100">{value}</p>
    </div>
  );
}
