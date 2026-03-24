import type { ArchiveStrategy } from '@emailibrium/types';
import type { AiSetupState } from './AISetup';

interface SetupCompleteProps {
  accountCount: number;
  aiSetup: AiSetupState | null;
  archiveStrategy: ArchiveStrategy;
  onLaunch: () => void;
  onSettings: () => void;
}

const STRATEGY_LABELS: Record<ArchiveStrategy, string> = {
  instant: 'Instant',
  delayed: 'Delayed',
  manual: 'Manual',
};

function aiTierLabel(aiSetup: AiSetupState | null): string {
  if (!aiSetup) return 'Local AI ready (ONNX)';
  switch (aiSetup.tier) {
    case 'onnx':
      return 'Local AI ready (ONNX)';
    case 'ollama':
      return aiSetup.ollamaStatus === 'connected' ? 'Ollama connected' : 'Ollama selected';
    case 'cloud':
      return aiSetup.cloudProvider
        ? `${aiSetup.cloudProvider.charAt(0).toUpperCase() + aiSetup.cloudProvider.slice(1)} configured`
        : 'Cloud AI configured';
    default:
      return 'Local AI ready';
  }
}

function CheckIcon() {
  return (
    <svg
      viewBox="0 0 20 20"
      fill="currentColor"
      className="w-5 h-5 text-green-500"
      aria-hidden="true"
    >
      <path
        fillRule="evenodd"
        d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.857-9.809a.75.75 0 00-1.214-.882l-3.483 4.79-1.88-1.88a.75.75 0 10-1.06 1.061l2.5 2.5a.75.75 0 001.137-.089l4-5.5z"
        clipRule="evenodd"
      />
    </svg>
  );
}

function AmberIcon() {
  return (
    <svg
      viewBox="0 0 20 20"
      fill="currentColor"
      className="w-5 h-5 text-amber-500"
      aria-hidden="true"
    >
      <path
        fillRule="evenodd"
        d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-8-5a.75.75 0 01.75.75v4.5a.75.75 0 01-1.5 0v-4.5A.75.75 0 0110 5zm0 10a1 1 0 100-2 1 1 0 000 2z"
        clipRule="evenodd"
      />
    </svg>
  );
}

export function SetupComplete({
  accountCount,
  aiSetup,
  archiveStrategy,
  onLaunch,
  onSettings,
}: SetupCompleteProps) {
  const emailConfigured = accountCount > 0;
  const aiConfigured =
    !aiSetup ||
    aiSetup.tier === 'onnx' ||
    aiSetup.ollamaStatus === 'connected' ||
    aiSetup.tier === 'cloud';
  const hasAmber = !emailConfigured;

  return (
    <div className="max-w-lg mx-auto space-y-6">
      <div className="text-center space-y-2">
        <div
          className="mx-auto flex items-center justify-center w-16 h-16 rounded-full bg-green-100 dark:bg-green-900/30"
          aria-hidden="true"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth={1.5}
            className="w-8 h-8 text-green-600 dark:text-green-400"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
            />
          </svg>
        </div>
        <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Setup Complete</h2>
        <p className="text-sm text-gray-500 dark:text-gray-400">
          Here is a summary of your configuration.
        </p>
      </div>

      {/* Summary card */}
      <div
        className="rounded-lg border border-gray-200 bg-white divide-y divide-gray-200
          dark:bg-gray-800 dark:border-gray-700 dark:divide-gray-700"
        role="list"
        aria-label="Setup summary"
      >
        {/* Email accounts */}
        <div className="flex items-center gap-3 px-4 py-3" role="listitem">
          {emailConfigured ? <CheckIcon /> : <AmberIcon />}
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium text-gray-900 dark:text-gray-100">Email Accounts</p>
            <p className="text-xs text-gray-500 dark:text-gray-400">
              {emailConfigured
                ? `${accountCount} account${accountCount !== 1 ? 's' : ''} connected`
                : 'None yet — configure later in Settings'}
            </p>
          </div>
        </div>

        {/* AI tier */}
        <div className="flex items-center gap-3 px-4 py-3" role="listitem">
          {aiConfigured ? <CheckIcon /> : <AmberIcon />}
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium text-gray-900 dark:text-gray-100">AI Engine</p>
            <p className="text-xs text-gray-500 dark:text-gray-400">{aiTierLabel(aiSetup)}</p>
          </div>
        </div>

        {/* Archive strategy */}
        <div className="flex items-center gap-3 px-4 py-3" role="listitem">
          <CheckIcon />
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium text-gray-900 dark:text-gray-100">Archive Strategy</p>
            <p className="text-xs text-gray-500 dark:text-gray-400">
              {STRATEGY_LABELS[archiveStrategy]}
            </p>
          </div>
        </div>
      </div>

      {/* Actions */}
      <div className="flex flex-col items-center gap-3 pt-2">
        <button
          type="button"
          onClick={onLaunch}
          className="w-full max-w-xs px-8 py-3 rounded-lg bg-indigo-600 text-white font-medium
            hover:bg-indigo-700 transition-colors"
        >
          Launch Emailibrium
        </button>
        {hasAmber && (
          <button
            type="button"
            onClick={onSettings}
            className="text-sm text-gray-500 hover:text-gray-700 dark:text-gray-400
              dark:hover:text-gray-200 transition-colors"
          >
            Go to Settings to finish setup
          </button>
        )}
      </div>
    </div>
  );
}
