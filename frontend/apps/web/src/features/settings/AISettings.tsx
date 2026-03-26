import { useSettings, type LlmProvider } from './hooks/useSettings';
import { useQuery } from '@tanstack/react-query';
import { useState, useMemo } from 'react';
import { ModelDownloadProgress } from './components/ModelDownloadProgress';

// ---------------------------------------------------------------------------
// Embedding models grouped by provider
// ---------------------------------------------------------------------------

type EmbeddingProvider = 'onnx' | 'ollama' | 'openai';

interface EmbeddingModelOption {
  value: string;
  label: string;
  provider: EmbeddingProvider;
  dimensions: number;
  description: string;
}

const EMBEDDING_MODELS: EmbeddingModelOption[] = [
  // ONNX (local, privacy-first — ADR-002 default)
  {
    value: 'all-MiniLM-L6-v2',
    label: 'all-MiniLM-L6-v2',
    provider: 'onnx',
    dimensions: 384,
    description: 'Fast, lightweight English embedding (22M params). Best size/quality trade-off.',
  },
  {
    value: 'bge-small-en-v1.5',
    label: 'bge-small-en-v1.5',
    provider: 'onnx',
    dimensions: 384,
    description: 'Higher quality English embedding with longer context (33M params).',
  },
  {
    value: 'bge-base-en-v1.5',
    label: 'bge-base-en-v1.5',
    provider: 'onnx',
    dimensions: 768,
    description: 'High quality English embedding. Requires more memory (109M params).',
  },
  // Ollama (local, requires running Ollama)
  {
    value: 'nomic-embed-text',
    label: 'nomic-embed-text',
    provider: 'ollama',
    dimensions: 768,
    description: 'General-purpose embedding via Ollama. Requires Ollama running locally.',
  },
  {
    value: 'mxbai-embed-large',
    label: 'mxbai-embed-large',
    provider: 'ollama',
    dimensions: 1024,
    description: 'High quality embedding via Ollama. Larger model, better for complex queries.',
  },
  // OpenAI (cloud, requires API key)
  {
    value: 'text-embedding-3-small',
    label: 'text-embedding-3-small',
    provider: 'openai',
    dimensions: 1536,
    description: 'OpenAI cloud embedding. Fast and cost-effective. Requires API key.',
  },
  {
    value: 'text-embedding-3-large',
    label: 'text-embedding-3-large',
    provider: 'openai',
    dimensions: 3072,
    description: 'OpenAI highest quality cloud embedding. Requires API key.',
  },
];

const EMBEDDING_PROVIDERS: { value: EmbeddingProvider; label: string; badge?: string }[] = [
  { value: 'onnx', label: 'Built-in (ONNX)', badge: 'Recommended' },
  { value: 'ollama', label: 'Ollama (Local)' },
  { value: 'openai', label: 'OpenAI (Cloud)' },
];

// ---------------------------------------------------------------------------
// LLM providers and their models
// ---------------------------------------------------------------------------

interface LlmModelOption {
  value: string;
  label: string;
  description: string;
}

const LLM_PROVIDERS: {
  value: LlmProvider;
  label: string;
  description: string;
  badge?: string;
}[] = [
  {
    value: 'none',
    label: 'None (Rule-based)',
    description: 'Keyword and domain heuristics only — no AI model. Fastest, but less accurate.',
  },
  {
    value: 'builtin',
    label: 'Built-in (Local)',
    description: 'Run a small LLM locally — no external service needed. Downloads a ~350 MB model.',
    badge: 'Default',
  },
  {
    value: 'local',
    label: 'Local (Ollama)',
    description: 'Run models locally via Ollama for privacy-preserving AI.',
  },
  { value: 'openai', label: 'OpenAI', description: 'GPT-4o and GPT-4o-mini. Requires API key.' },
  {
    value: 'anthropic',
    label: 'Anthropic',
    description: 'Claude models. Requires API key.',
  },
];

const LLM_MODELS: Record<LlmProvider, LlmModelOption[]> = {
  none: [],
  builtin: [
    {
      value: 'qwen2.5-0.5b-q4km',
      label: 'Qwen 2.5 0.5B',
      description: 'Fast classification (0.5B params, ~350 MB)',
    },
    {
      value: 'smollm2-360m-q4km',
      label: 'SmolLM2 360M',
      description: 'Ultra-light classification (360M params, ~250 MB)',
    },
    {
      value: 'smollm2-1.7b-q4km',
      label: 'SmolLM2 1.7B',
      description: 'Better quality + basic chat (1.7B params, ~1 GB)',
    },
    {
      value: 'llama3.2-3b-q4km',
      label: 'Llama 3.2 3B',
      description: 'High-quality chat (3B params, ~1.8 GB)',
    },
    {
      value: 'phi3.5-mini-q4km',
      label: 'Phi 3.5 mini',
      description: 'Best quality, higher resources (3.8B params, ~2.3 GB)',
    },
  ],
  local: [
    { value: 'llama3.2:1b', label: 'Llama 3.2 1B', description: 'Fast classification (1B params)' },
    { value: 'llama3.2:3b', label: 'Llama 3.2 3B', description: 'Better quality chat (3B params)' },
    { value: 'mistral:7b', label: 'Mistral 7B', description: 'Strong general-purpose (7B params)' },
    { value: 'gemma2:2b', label: 'Gemma 2 2B', description: 'Compact and efficient (2B params)' },
  ],
  openai: [
    { value: 'gpt-4o-mini', label: 'GPT-4o mini', description: 'Fast and cost-effective' },
    { value: 'gpt-4o', label: 'GPT-4o', description: 'Highest quality' },
  ],
  anthropic: [
    {
      value: 'claude-sonnet-4-6',
      label: 'Claude Sonnet 4',
      description: 'Balanced speed and quality',
    },
    {
      value: 'claude-haiku-4-5-20251001',
      label: 'Claude Haiku 4.5',
      description: 'Fast and cost-effective',
    },
  ],
};

// ---------------------------------------------------------------------------
// Ollama model discovery
// ---------------------------------------------------------------------------

interface OllamaTagsResponse {
  models: { name: string; size: number; details?: { parameter_size?: string } }[];
}

async function fetchOllamaModels(baseUrl: string): Promise<LlmModelOption[]> {
  const res = await fetch(`${baseUrl}/api/tags`, { signal: AbortSignal.timeout(3000) });
  if (!res.ok) throw new Error(`Ollama returned ${res.status}`);
  const data: OllamaTagsResponse = await res.json();
  return data.models.map((m) => ({
    value: m.name,
    label: m.name,
    description: m.details?.parameter_size
      ? `${m.details.parameter_size} params, ${(m.size / 1e9).toFixed(1)}GB`
      : `${(m.size / 1e9).toFixed(1)}GB on disk`,
  }));
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function AISettings() {
  const {
    embeddingModel,
    llmProvider,
    openaiApiKey,
    anthropicApiKey,
    ollamaBaseUrl,
    sonaLearningEnabled,
    learningRateSensitivity,
    setEmbeddingModel,
    setLlmProvider,
    setOpenaiApiKey,
    setAnthropicApiKey,
    setOllamaBaseUrl,
    setSonaLearningEnabled,
    setLearningRateSensitivity,
  } = useSettings();

  const [isResetting, setIsResetting] = useState(false);
  const [llmModel, setLlmModel] = useState(() => LLM_MODELS[llmProvider]?.[0]?.value ?? '');

  // Derive the current embedding provider from the selected model
  const embeddingProvider = useMemo(() => {
    const found = EMBEDDING_MODELS.find((m) => m.value === embeddingModel);
    return found?.provider ?? 'onnx';
  }, [embeddingModel]);

  // Fetch live Ollama models when the provider is local
  const ollamaModelsQuery = useQuery({
    queryKey: ['ollama-models', ollamaBaseUrl],
    queryFn: () => fetchOllamaModels(ollamaBaseUrl),
    enabled: llmProvider === 'local' || embeddingProvider === 'ollama',
    staleTime: 30_000,
    retry: false,
  });

  // Filter embedding models by selected provider
  const filteredEmbeddingModels = useMemo(
    () => EMBEDDING_MODELS.filter((m) => m.provider === embeddingProvider),
    [embeddingProvider],
  );

  // Current embedding model details
  const currentEmbeddingModel = EMBEDDING_MODELS.find((m) => m.value === embeddingModel);

  // Available LLM models for the selected provider.
  // When Ollama is selected, prefer live-fetched models over the hardcoded fallback list.
  const availableLlmModels = useMemo(() => {
    if (llmProvider === 'local' && ollamaModelsQuery.data && ollamaModelsQuery.data.length > 0) {
      return ollamaModelsQuery.data;
    }
    return LLM_MODELS[llmProvider] ?? [];
  }, [llmProvider, ollamaModelsQuery.data]);

  function handleEmbeddingProviderChange(provider: EmbeddingProvider) {
    const firstModel = EMBEDDING_MODELS.find((m) => m.provider === provider);
    if (firstModel) {
      setEmbeddingModel(firstModel.value);
    }
  }

  function handleLlmProviderChange(provider: LlmProvider) {
    setLlmProvider(provider);
    const models = LLM_MODELS[provider] ?? [];
    if (models.length > 0) {
      setLlmModel(models[0]!.value);
    } else {
      setLlmModel('');
    }
  }

  async function handleResetPreferences() {
    setIsResetting(true);
    try {
      await new Promise((resolve) => setTimeout(resolve, 500));
      setLearningRateSensitivity(0.5);
    } finally {
      setIsResetting(false);
    }
  }

  return (
    <div className="space-y-8">
      <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100">
        AI / LLM Settings
      </h3>

      {/* ── Embedding Provider ── */}
      <fieldset className="space-y-2">
        <legend className="text-sm font-medium text-gray-700 dark:text-gray-300">
          Embedding Provider
        </legend>
        <p className="text-xs text-gray-500 dark:text-gray-400 mb-2">
          How email vectors are generated. Built-in ONNX runs entirely on your machine.
        </p>
        <div className="flex flex-wrap gap-2 max-w-lg">
          {EMBEDDING_PROVIDERS.map((ep) => (
            <button
              key={ep.value}
              type="button"
              onClick={() => handleEmbeddingProviderChange(ep.value)}
              className={`relative px-3 py-1.5 rounded-lg border text-sm font-medium transition-all ${
                embeddingProvider === ep.value
                  ? 'border-indigo-500 bg-indigo-50 text-indigo-700 dark:bg-indigo-900/20 dark:text-indigo-300 dark:border-indigo-400'
                  : 'border-gray-200 text-gray-600 hover:border-gray-300 dark:border-gray-700 dark:text-gray-400 dark:hover:border-gray-600'
              }`}
            >
              {ep.label}
              {ep.badge && (
                <span className="ml-1.5 inline-flex items-center rounded-full bg-green-100 px-1.5 py-0.5 text-[10px] font-medium text-green-700 dark:bg-green-900/30 dark:text-green-400">
                  {ep.badge}
                </span>
              )}
            </button>
          ))}
        </div>
      </fieldset>

      {/* ── Embedding Model ── */}
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
          {filteredEmbeddingModels.map((m) => (
            <option key={m.value} value={m.value}>
              {m.label} ({m.dimensions}d)
            </option>
          ))}
        </select>
        {currentEmbeddingModel && (
          <p className="text-xs text-gray-500 dark:text-gray-400">
            {currentEmbeddingModel.description}
          </p>
        )}
      </div>

      {/* ── Embedding Provider Config (API key / URL) ── */}
      {embeddingProvider === 'openai' && (
        <ApiKeyInput
          id="embedding-openai-key"
          label="OpenAI API Key (Embeddings)"
          value={openaiApiKey}
          onChange={setOpenaiApiKey}
          placeholder="sk-..."
          helpText="Used for text-embedding-3 models. Stored locally only."
        />
      )}
      {embeddingProvider === 'ollama' && (
        <UrlInput
          id="embedding-ollama-url"
          label="Ollama Base URL"
          value={ollamaBaseUrl}
          onChange={setOllamaBaseUrl}
          placeholder="http://localhost:11434"
          helpText="URL of your local Ollama instance."
        />
      )}

      {/* ── LLM Provider ── */}
      <fieldset className="space-y-2">
        <legend className="text-sm font-medium text-gray-700 dark:text-gray-300">
          LLM Provider
        </legend>
        <p className="text-xs text-gray-500 dark:text-gray-400 mb-2">
          Used for classification fallback and chat. Rule-based mode needs no external service.
        </p>
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
                onChange={() => handleLlmProviderChange(provider.value)}
                className="mt-0.5 h-4 w-4 text-indigo-600 border-gray-300 focus:ring-indigo-500
                  dark:border-gray-600"
              />
              <div>
                <span className="block text-sm font-medium text-gray-900 dark:text-gray-100">
                  {provider.label}
                  {provider.badge && (
                    <span className="ml-1.5 inline-flex items-center rounded-full bg-green-100 px-1.5 py-0.5 text-[10px] font-medium text-green-700 dark:bg-green-900/30 dark:text-green-400">
                      {provider.badge}
                    </span>
                  )}
                </span>
                <span className="block text-xs text-gray-500 dark:text-gray-400">
                  {provider.description}
                </span>
              </div>
            </label>
          );
        })}
      </fieldset>

      {/* ── LLM Model (shown only when provider has models) ── */}
      {availableLlmModels.length > 0 && (
        <div className="space-y-1">
          <label
            htmlFor="llm-model"
            className="block text-sm font-medium text-gray-700 dark:text-gray-300"
          >
            LLM Model
          </label>
          <select
            id="llm-model"
            value={llmModel}
            onChange={(e) => setLlmModel(e.target.value)}
            className="w-full max-w-sm rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm
              focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
              dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
          >
            {availableLlmModels.map((m) => (
              <option key={m.value} value={m.value}>
                {m.label}
              </option>
            ))}
          </select>
          <p className="text-xs text-gray-500 dark:text-gray-400">
            {availableLlmModels.find((m) => m.value === llmModel)?.description ?? ''}
          </p>
        </div>
      )}

      {/* ── LLM Provider Config (API key / URL) ── */}
      {llmProvider === 'openai' && (
        <ApiKeyInput
          id="llm-openai-key"
          label="OpenAI API Key"
          value={openaiApiKey}
          onChange={setOpenaiApiKey}
          placeholder="sk-..."
          helpText="Required for GPT models. Stored locally, never sent to Emailibrium servers."
        />
      )}
      {llmProvider === 'anthropic' && (
        <ApiKeyInput
          id="llm-anthropic-key"
          label="Anthropic API Key"
          value={anthropicApiKey}
          onChange={setAnthropicApiKey}
          placeholder="sk-ant-..."
          helpText="Required for Claude models. Stored locally, never sent to Emailibrium servers."
        />
      )}
      {llmProvider === 'builtin' && (
        <div className="space-y-2 max-w-sm">
          <ModelDownloadProgress modelId={llmModel || 'qwen2.5-0.5b-q4km'} />
          <p className="text-xs text-gray-500 dark:text-gray-400">
            Model runs entirely on your machine. No data leaves your device.
          </p>
        </div>
      )}
      {llmProvider === 'local' && (
        <div className="space-y-2">
          <UrlInput
            id="llm-ollama-url"
            label="Ollama Base URL"
            value={ollamaBaseUrl}
            onChange={setOllamaBaseUrl}
            placeholder="http://localhost:11434"
            helpText="URL of your local Ollama instance."
          />
          <OllamaStatus query={ollamaModelsQuery} />
        </div>
      )}

      {/* ── SONA Learning toggle ── */}
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

      {/* ── Learning rate sensitivity ── */}
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

      {/* ── Reset learned preferences ── */}
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

// ---------------------------------------------------------------------------
// Reusable input components
// ---------------------------------------------------------------------------

interface ApiKeyInputProps {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  helpText: string;
}

function ApiKeyInput({ id, label, value, onChange, placeholder, helpText }: ApiKeyInputProps) {
  const [visible, setVisible] = useState(false);

  return (
    <div className="space-y-1 max-w-sm">
      <label htmlFor={id} className="block text-sm font-medium text-gray-700 dark:text-gray-300">
        {label}
      </label>
      <div className="relative">
        <input
          id={id}
          type={visible ? 'text' : 'password'}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          autoComplete="off"
          className="w-full rounded-lg border border-gray-300 bg-white px-3 py-2 pr-16 text-sm font-mono
            focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
            dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
        />
        <button
          type="button"
          onClick={() => setVisible(!visible)}
          className="absolute right-2 top-1/2 -translate-y-1/2 px-2 py-0.5 text-xs text-gray-500
            hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
        >
          {visible ? 'Hide' : 'Show'}
        </button>
      </div>
      <p className="text-xs text-gray-500 dark:text-gray-400">{helpText}</p>
      {value && <p className="text-xs text-green-600 dark:text-green-400">Key saved.</p>}
    </div>
  );
}

interface UrlInputProps {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  helpText: string;
}

function UrlInput({ id, label, value, onChange, placeholder, helpText }: UrlInputProps) {
  return (
    <div className="space-y-1 max-w-sm">
      <label htmlFor={id} className="block text-sm font-medium text-gray-700 dark:text-gray-300">
        {label}
      </label>
      <input
        id={id}
        type="url"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm font-mono
          focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
          dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
      />
      <p className="text-xs text-gray-500 dark:text-gray-400">{helpText}</p>
    </div>
  );
}

interface OllamaStatusProps {
  query: { isLoading: boolean; isError: boolean; data?: LlmModelOption[]; error: Error | null };
}

function OllamaStatus({ query }: OllamaStatusProps) {
  if (query.isLoading) {
    return (
      <p className="text-xs text-yellow-600 dark:text-yellow-400 max-w-sm">
        Connecting to Ollama...
      </p>
    );
  }
  if (query.isError) {
    return (
      <p className="text-xs text-red-600 dark:text-red-400 max-w-sm">
        Cannot reach Ollama. Make sure it is running and the URL is correct.
      </p>
    );
  }
  if (query.data) {
    return (
      <p className="text-xs text-green-600 dark:text-green-400 max-w-sm">
        Connected. {query.data.length} model{query.data.length !== 1 ? 's' : ''} available.
      </p>
    );
  }
  return null;
}
