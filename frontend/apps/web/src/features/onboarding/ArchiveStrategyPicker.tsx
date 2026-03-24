import type { ArchiveStrategy } from '@emailibrium/types';

interface StrategyOption {
  value: ArchiveStrategy;
  label: string;
  description: string;
}

const STRATEGIES: StrategyOption[] = [
  {
    value: 'instant',
    label: 'Instant',
    description: 'Archive immediately. True zero inbox.',
  },
  {
    value: 'delayed',
    label: 'Delayed',
    description: 'Archive after 60s. Mobile notifications work.',
  },
  {
    value: 'manual',
    label: 'Manual',
    description: 'Never auto-archive. You mark Done.',
  },
];

interface ArchiveStrategyPickerProps {
  value: ArchiveStrategy;
  onChange: (strategy: ArchiveStrategy) => void;
}

export function ArchiveStrategyPicker({ value, onChange }: ArchiveStrategyPickerProps) {
  return (
    <fieldset className="max-w-md mx-auto space-y-3">
      <legend className="text-lg font-semibold text-gray-900 dark:text-gray-100 text-center mb-1">
        Choose your archive strategy
      </legend>
      <p className="text-sm text-gray-500 dark:text-gray-400 text-center mb-4">
        How should Emailibrium handle processed emails?
      </p>
      {STRATEGIES.map((strategy) => {
        const isSelected = value === strategy.value;
        return (
          <label
            key={strategy.value}
            className={`flex items-start gap-3 p-4 rounded-lg border-2 cursor-pointer transition-all ${
              isSelected
                ? 'border-indigo-500 bg-indigo-50 dark:bg-indigo-900/20 dark:border-indigo-400'
                : 'border-gray-200 bg-white hover:border-gray-300 dark:bg-gray-800 dark:border-gray-700 dark:hover:border-gray-600'
            }`}
          >
            <input
              type="radio"
              name="archiveStrategy"
              value={strategy.value}
              checked={isSelected}
              onChange={() => onChange(strategy.value)}
              className="mt-0.5 h-4 w-4 text-indigo-600 border-gray-300 focus:ring-indigo-500
                dark:border-gray-600 dark:bg-gray-700"
            />
            <div>
              <span className="block text-sm font-medium text-gray-900 dark:text-gray-100">
                {strategy.label}
              </span>
              <span className="block text-sm text-gray-500 dark:text-gray-400 mt-0.5">
                {strategy.description}
              </span>
            </div>
          </label>
        );
      })}
    </fieldset>
  );
}
