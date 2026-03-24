import { useSettings, type LlmProvider } from './hooks/useSettings';
import { useState } from 'react';

const EMBEDDING_MODELS = [
  { value: 'text-embedding-3-small', label: 'text-embedding-3-small (OpenAI)' },
  { value: 'text-embedding-3-large', label: 'text-embedding-3-large (OpenAI)' },
  { value: 'nomic-embed-text', label: 'nomic-embed-text (Local)' },
  { value: 'all-MiniLM-L6-v2', label: 'all-MiniLM-L6-v2 (Local)' },
];

const LLM_PROVIDERS: { value: LlmProvider; label: string; description: string }[] = [
  {
    value: 'local',
    label: 'Local (Ollama)',
    description: 'Run models locally for maximum privacy',
  },
  { value: 'openai', label: 'OpenAI', description: 'GPT-4o and GPT-4o-mini' },
  { value: 'anthropic', label: 'Anthropic', description: 'Claude models' },
];

export function AISettings() {
  const {
    embeddingModel,
    llmProvider,
    sonaLearningEnabled,
    learningRateSensitivity,
    setEmbeddingModel,
    setLlmProvider,
    setSonaLearningEnabled,
    setLearningRateSensitivity,
  } = useSettings();

  const [isResetting, setIsResetting] = useState(false);

  async function handleResetPreferences() {
    setIsResetting(true);
    try {
      // In production, call the API to reset SONA learned preferences.
      await new Promise((resolve) => setTimeout(resolve, 500));
      setLearningRateSensitivity(0.5);
    } finally {
      setIsResetting(false);
    }
  }

  return (
    <div className="space-y-6">
      <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100">
        AI / LLM Settings
      </h3>

      {/* Embedding model */}
      <div className="space-y-1">
        <label
          htmlFor="embedding-model"
          className="block text-sm font-medium text-gray-700 dark:text-gray-300"
        >
          Embedding Model
        </label>
        <select
          id="embedding-model"
          value={embeddingModel}
          onChange={(e) => setEmbeddingModel(e.target.value)}
          className="w-full max-w-sm rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm
            focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
            dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
        >
          {EMBEDDING_MODELS.map((m) => (
            <option key={m.value} value={m.value}>
              {m.label}
            </option>
          ))}
        </select>
        <p className="text-xs text-gray-500 dark:text-gray-400">
          The model used to generate email vector embeddings.
        </p>
      </div>

      {/* LLM provider */}
      <fieldset className="space-y-2">
        <legend className="text-sm font-medium text-gray-700 dark:text-gray-300">
          LLM Provider
        </legend>
        {LLM_PROVIDERS.map((provider) => {
          const isSelected = llmProvider === provider.value;
          return (
            <label
              key={provider.value}
              className={`flex items-start gap-3 p-3 rounded-lg border-2 cursor-pointer transition-all max-w-sm ${
                isSelected
                  ? 'border-indigo-500 bg-indigo-50 dark:bg-indigo-900/20 dark:border-indigo-400'
                  : 'border-gray-200 bg-white hover:border-gray-300 dark:bg-gray-800 dark:border-gray-700'
              }`}
            >
              <input
                type="radio"
                name="llmProvider"
                value={provider.value}
                checked={isSelected}
                onChange={() => setLlmProvider(provider.value)}
                className="mt-0.5 h-4 w-4 text-indigo-600 border-gray-300 focus:ring-indigo-500
                  dark:border-gray-600"
              />
              <div>
                <span className="block text-sm font-medium text-gray-900 dark:text-gray-100">
                  {provider.label}
                </span>
                <span className="block text-xs text-gray-500 dark:text-gray-400">
                  {provider.description}
                </span>
              </div>
            </label>
          );
        })}
      </fieldset>

      {/* SONA learning toggle */}
      <div className="flex items-center justify-between max-w-sm">
        <div>
          <span className="block text-sm font-medium text-gray-700 dark:text-gray-300">
            SONA Learning
          </span>
          <span className="text-xs text-gray-500 dark:text-gray-400">
            Continuously adapt to your email preferences
          </span>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={sonaLearningEnabled}
          onClick={() => setSonaLearningEnabled(!sonaLearningEnabled)}
          className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2
            border-transparent transition-colors focus:outline-none focus:ring-2 focus:ring-indigo-500
            focus:ring-offset-2 ${
              sonaLearningEnabled ? 'bg-indigo-600' : 'bg-gray-200 dark:bg-gray-600'
            }`}
        >
          <span
            className={`pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white
              shadow ring-0 transition-transform ${
                sonaLearningEnabled ? 'translate-x-5' : 'translate-x-0'
              }`}
          />
        </button>
      </div>

      {/* Learning rate sensitivity */}
      <div className="space-y-1 max-w-sm">
        <label
          htmlFor="learning-rate"
          className="block text-sm font-medium text-gray-700 dark:text-gray-300"
        >
          Learning Rate Sensitivity
        </label>
        <div className="flex items-center gap-3">
          <input
            id="learning-rate"
            type="range"
            min={0}
            max={1}
            step={0.05}
            value={learningRateSensitivity}
            onChange={(e) => setLearningRateSensitivity(Number(e.target.value))}
            className="flex-1 h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer
              dark:bg-gray-700 accent-indigo-600"
            disabled={!sonaLearningEnabled}
          />
          <span className="text-sm text-gray-600 dark:text-gray-400 w-10 text-right tabular-nums">
            {learningRateSensitivity.toFixed(2)}
          </span>
        </div>
        <p className="text-xs text-gray-500 dark:text-gray-400">
          Higher values make the AI adapt faster to your feedback.
        </p>
      </div>

      {/* Reset learned preferences */}
      <div className="pt-4 border-t border-gray-200 dark:border-gray-700">
        <button
          type="button"
          onClick={handleResetPreferences}
          disabled={isResetting}
          className="px-4 py-2 rounded-lg border border-red-300 text-red-700 text-sm font-medium
            hover:bg-red-50 disabled:opacity-60 disabled:cursor-not-allowed transition-colors
            dark:border-red-700 dark:text-red-400 dark:hover:bg-red-900/20"
        >
          {isResetting ? 'Resetting...' : 'Reset Learned Preferences'}
        </button>
        <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
          This clears all SONA adaptations and starts fresh.
        </p>
      </div>
    </div>
  );
}
