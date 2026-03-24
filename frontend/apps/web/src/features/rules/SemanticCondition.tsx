interface SemanticConditionProps {
  type: 'similar-to' | 'about-topic';
  value: string;
  threshold: number;
  onChangeValue: (value: string) => void;
  onChangeThreshold: (threshold: number) => void;
}

export function SemanticCondition({
  type,
  value,
  threshold,
  onChangeValue,
  onChangeThreshold,
}: SemanticConditionProps) {
  return (
    <div className="rounded-md border border-indigo-200 bg-indigo-50 p-3 dark:border-indigo-800 dark:bg-indigo-900/20">
      <p className="mb-2 text-xs font-semibold uppercase tracking-wider text-indigo-600 dark:text-indigo-400">
        {type === 'similar-to' ? 'Semantic: Similar to Email' : 'Semantic: About Topic'}
      </p>

      {type === 'similar-to' ? (
        <div>
          <label
            htmlFor="semantic-email-picker"
            className="mb-1 block text-sm text-gray-600 dark:text-gray-300"
          >
            Select an email to match similar ones:
          </label>
          <input
            id="semantic-email-picker"
            type="text"
            value={value}
            onChange={(e) => onChangeValue(e.target.value)}
            placeholder="Search for an email by subject..."
            className="w-full rounded-md border border-gray-200 bg-white px-3 py-1.5 text-sm text-gray-900 outline-none focus:border-indigo-400 dark:border-gray-600 dark:bg-gray-700 dark:text-white"
          />
        </div>
      ) : (
        <div>
          <label
            htmlFor="semantic-topic"
            className="mb-1 block text-sm text-gray-600 dark:text-gray-300"
          >
            Describe the topic:
          </label>
          <input
            id="semantic-topic"
            type="text"
            value={value}
            onChange={(e) => onChangeValue(e.target.value)}
            placeholder='e.g. "quarterly financial reports"'
            className="w-full rounded-md border border-gray-200 bg-white px-3 py-1.5 text-sm text-gray-900 outline-none focus:border-indigo-400 dark:border-gray-600 dark:bg-gray-700 dark:text-white"
          />
        </div>
      )}

      {/* Threshold slider */}
      <div className="mt-3">
        <div className="flex items-center justify-between">
          <label
            htmlFor="similarity-threshold"
            className="text-xs text-gray-500 dark:text-gray-400"
          >
            Similarity threshold
          </label>
          <span className="text-xs font-medium text-indigo-600 dark:text-indigo-400">
            {(threshold * 100).toFixed(0)}%
          </span>
        </div>
        <input
          id="similarity-threshold"
          type="range"
          min={0}
          max={100}
          value={threshold * 100}
          onChange={(e) => onChangeThreshold(Number(e.target.value) / 100)}
          className="mt-1 w-full accent-indigo-600"
          aria-label="Similarity threshold"
        />
        <div className="flex justify-between text-[10px] text-gray-400">
          <span>Loose</span>
          <span>Strict</span>
        </div>
      </div>
    </div>
  );
}
