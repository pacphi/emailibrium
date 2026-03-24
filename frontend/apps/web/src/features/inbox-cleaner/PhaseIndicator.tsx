import type { IngestionPhase } from '@emailibrium/types';

interface PhaseConfig {
  key: IngestionPhase;
  label: string;
  icon: string;
}

const phases: PhaseConfig[] = [
  { key: 'syncing', label: 'Sync', icon: 'cloud_download' },
  { key: 'embedding', label: 'Embed', icon: 'hub' },
  { key: 'categorizing', label: 'Categorize', icon: 'label' },
  { key: 'clustering', label: 'Cluster', icon: 'bubble_chart' },
  { key: 'analyzing', label: 'Analyze', icon: 'analytics' },
  { key: 'complete', label: 'Done', icon: 'check_circle' },
];

type PhaseStatus = 'pending' | 'running' | 'complete';

function getPhaseStatus(phaseKey: IngestionPhase, currentPhase: IngestionPhase): PhaseStatus {
  const currentIndex = phases.findIndex((p) => p.key === currentPhase);
  const phaseIndex = phases.findIndex((p) => p.key === phaseKey);

  if (phaseIndex < currentIndex) return 'complete';
  if (phaseIndex === currentIndex) return currentPhase === 'complete' ? 'complete' : 'running';
  return 'pending';
}

interface PhaseIndicatorProps {
  currentPhase: IngestionPhase;
}

export function PhaseIndicator({ currentPhase }: PhaseIndicatorProps) {
  return (
    <div className="w-full">
      <div className="flex items-center justify-between">
        {phases.map((phase, index) => {
          const status = getPhaseStatus(phase.key, currentPhase);
          return (
            <div key={phase.key} className="flex items-center flex-1 last:flex-none">
              <div className="flex flex-col items-center">
                <div
                  className={`
                    w-10 h-10 rounded-full flex items-center justify-center text-sm font-medium
                    transition-all duration-300
                    ${
                      status === 'complete'
                        ? 'bg-green-500 text-white'
                        : status === 'running'
                          ? 'bg-blue-500 text-white animate-pulse ring-4 ring-blue-200 dark:ring-blue-800'
                          : 'bg-gray-200 text-gray-500 dark:bg-gray-700 dark:text-gray-400'
                    }
                  `}
                  aria-label={`${phase.label}: ${status}`}
                >
                  {status === 'complete' ? (
                    <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        strokeWidth={2}
                        d="M5 13l4 4L19 7"
                      />
                    </svg>
                  ) : (
                    <span className="text-xs">{index + 1}</span>
                  )}
                </div>
                <span
                  className={`mt-2 text-xs font-medium ${
                    status === 'running'
                      ? 'text-blue-600 dark:text-blue-400'
                      : status === 'complete'
                        ? 'text-green-600 dark:text-green-400'
                        : 'text-gray-400 dark:text-gray-500'
                  }`}
                >
                  {phase.label}
                </span>
              </div>
              {index < phases.length - 1 && (
                <div
                  className={`flex-1 h-0.5 mx-2 mt-[-1rem] transition-colors duration-300 ${
                    getPhaseStatus(phases[index + 1]!.key, currentPhase) !== 'pending'
                      ? 'bg-green-500'
                      : status === 'running' || status === 'complete'
                        ? 'bg-blue-300 dark:bg-blue-700'
                        : 'bg-gray-200 dark:bg-gray-700'
                  }`}
                />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
