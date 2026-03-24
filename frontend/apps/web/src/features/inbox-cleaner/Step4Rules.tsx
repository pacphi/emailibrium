import type { ArchiveStrategy } from '@emailibrium/types';
import type { SuggestedRule } from './hooks/useInboxCleaner';

interface Step4RulesProps {
  suggestedRules: SuggestedRule[];
  onToggleRule: (ruleId: string) => void;
  archiveStrategy: ArchiveStrategy;
  onArchiveStrategyChange: (strategy: ArchiveStrategy) => void;
}

const strategyDescriptions: Record<ArchiveStrategy, string> = {
  instant: 'Archive emails immediately when rules match. Best for aggressive cleanup.',
  delayed: 'Wait 24 hours before archiving. Gives you time to review.',
  manual: 'Mark for archiving but require manual confirmation. Most conservative.',
};

export function Step4Rules({
  suggestedRules,
  onToggleRule,
  archiveStrategy,
  onArchiveStrategyChange,
}: Step4RulesProps) {
  const enabledCount = suggestedRules.filter((r) => r.enabled).length;
  const totalEstimatedMatches = suggestedRules
    .filter((r) => r.enabled)
    .reduce((sum, r) => sum + r.estimatedMatches, 0);

  return (
    <div className="space-y-6">
      {/* Suggested rules */}
      <div>
        <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 mb-1">
          Suggested Rules
        </h3>
        <p className="text-xs text-gray-500 dark:text-gray-400 mb-4">
          Based on your cleanup patterns, we suggest these automated rules.
        </p>

        {suggestedRules.length === 0 ? (
          <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-6 text-center">
            <p className="text-sm text-gray-500 dark:text-gray-400">
              No rules suggested yet. Complete steps 2 and 3 to generate suggestions.
            </p>
          </div>
        ) : (
          <div className="space-y-2">
            {suggestedRules.map((rule) => (
              <label
                key={rule.id}
                className={`flex items-start gap-3 px-4 py-3 rounded-lg border cursor-pointer transition-colors ${
                  rule.enabled
                    ? 'border-blue-300 bg-blue-50 dark:border-blue-700 dark:bg-blue-900/20'
                    : 'border-gray-200 bg-white hover:border-gray-300 dark:border-gray-700 dark:bg-gray-800 dark:hover:border-gray-600'
                }`}
              >
                <input
                  type="checkbox"
                  checked={rule.enabled}
                  onChange={() => onToggleRule(rule.id)}
                  className="mt-0.5 h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500"
                />
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium text-gray-900 dark:text-gray-100">
                    {rule.description}
                  </p>
                  <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                    Estimated matches: {rule.estimatedMatches.toLocaleString()} emails
                  </p>
                </div>
              </label>
            ))}
          </div>
        )}

        {enabledCount > 0 && (
          <div className="mt-3 rounded-md bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 px-4 py-2">
            <p className="text-xs text-blue-700 dark:text-blue-300">
              <span className="font-semibold">{enabledCount}</span> rule
              {enabledCount !== 1 ? 's' : ''} enabled, covering approximately{' '}
              <span className="font-semibold">{totalEstimatedMatches.toLocaleString()}</span> emails
              going forward.
            </p>
          </div>
        )}
      </div>

      {/* Archive strategy */}
      <div>
        <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 mb-1">
          Archive Strategy
        </h3>
        <p className="text-xs text-gray-500 dark:text-gray-400 mb-4">
          Choose how aggressively rules should apply.
        </p>

        <div className="space-y-2">
          {(['instant', 'delayed', 'manual'] as ArchiveStrategy[]).map((strategy) => (
            <label
              key={strategy}
              className={`flex items-start gap-3 px-4 py-3 rounded-lg border cursor-pointer transition-colors ${
                archiveStrategy === strategy
                  ? 'border-indigo-300 bg-indigo-50 dark:border-indigo-700 dark:bg-indigo-900/20'
                  : 'border-gray-200 bg-white hover:border-gray-300 dark:border-gray-700 dark:bg-gray-800 dark:hover:border-gray-600'
              }`}
            >
              <input
                type="radio"
                name="archiveStrategy"
                value={strategy}
                checked={archiveStrategy === strategy}
                onChange={() => onArchiveStrategyChange(strategy)}
                className="mt-0.5 h-4 w-4 border-gray-300 text-indigo-600 focus:ring-indigo-500"
              />
              <div>
                <p className="text-sm font-medium text-gray-900 dark:text-gray-100 capitalize">
                  {strategy}
                </p>
                <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                  {strategyDescriptions[strategy]}
                </p>
              </div>
            </label>
          ))}
        </div>
      </div>
    </div>
  );
}
