import { useState, useEffect, useCallback } from 'react';

type AiTier = 'onnx' | 'builtin' | 'ollama' | 'cloud';
type CloudProvider = 'openai' | 'anthropic' | 'gemini';

interface AiSetupState {
  tier: AiTier;
  ollamaStatus: 'checking' | 'connected' | 'unavailable';
  cloudProvider: CloudProvider | null;
  cloudApiKey: string;
}

interface AISetupProps {
  onContinue: (state: AiSetupState) => void;
  onSkip: () => void;
}

export type { AiTier, AiSetupState };

export function AISetup({ onContinue, onSkip }: AISetupProps) {
  const [tier, setTier] = useState<AiTier>('builtin');
  const [ollamaStatus, setOllamaStatus] = useState<'checking' | 'connected' | 'unavailable'>(
    'checking',
  );
  const [cloudExpanded, setCloudExpanded] = useState(false);
  const [cloudProvider, setCloudProvider] = useState<CloudProvider>('openai');
  const [cloudApiKey, setCloudApiKey] = useState('');
  const [keyVisible, setKeyVisible] = useState(false);

  // Check if Ollama is reachable
  useEffect(() => {
    const controller = new AbortController();
    setOllamaStatus('checking');

    fetch('/api/v1/ai/health', { signal: controller.signal })
      .then((res) => {
        if (res.ok) {
          setOllamaStatus('connected');
        } else {
          setOllamaStatus('unavailable');
        }
      })
      .catch(() => {
        setOllamaStatus('unavailable');
      });

    return () => controller.abort();
  }, []);

  const handleContinue = useCallback(() => {
    onContinue({
      tier,
      ollamaStatus,
      cloudProvider: tier === 'cloud' ? cloudProvider : null,
      cloudApiKey: tier === 'cloud' ? cloudApiKey : '',
    });
  }, [tier, ollamaStatus, cloudProvider, cloudApiKey, onContinue]);

  return (
    <div className="max-w-lg mx-auto space-y-6">
      <div className="text-center space-y-2">
        <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">AI Configuration</h2>
        <p className="text-sm text-gray-500 dark:text-gray-400 max-w-md mx-auto">
          Emailibrium uses AI to classify, prioritize, and organize your email. Local AI works out
          of the box — no setup required.
        </p>
      </div>

      <div className="space-y-3">
        {/* Tier 1: ONNX (Local AI) */}
        <button
          type="button"
          onClick={() => setTier('onnx')}
          className={`w-full flex items-start gap-4 p-4 rounded-lg border-2 text-left transition-all ${
            tier === 'onnx'
              ? 'border-indigo-500 bg-indigo-50 dark:bg-indigo-900/20 dark:border-indigo-400'
              : 'border-gray-200 bg-white hover:border-gray-300 dark:bg-gray-800 dark:border-gray-700 dark:hover:border-gray-600'
          }`}
          aria-pressed={tier === 'onnx'}
        >
          <div className="shrink-0 mt-0.5">
            <span
              className="flex items-center justify-center w-8 h-8 rounded-full bg-green-100 dark:bg-green-900/30"
              aria-hidden="true"
            >
              <svg
                viewBox="0 0 20 20"
                fill="currentColor"
                className="w-5 h-5 text-green-600 dark:text-green-400"
              >
                <path
                  fillRule="evenodd"
                  d="M16.704 4.153a.75.75 0 01.143 1.052l-8 10.5a.75.75 0 01-1.127.075l-4.5-4.5a.75.75 0 011.06-1.06l3.894 3.893 7.48-9.817a.75.75 0 011.05-.143z"
                  clipRule="evenodd"
                />
              </svg>
            </span>
          </div>
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                Local AI (ONNX)
              </span>
              <span className="inline-flex items-center gap-1 rounded-full bg-green-100 px-2 py-0.5 text-[10px] font-medium text-green-700 dark:bg-green-900/30 dark:text-green-400">
                <span className="w-1.5 h-1.5 rounded-full bg-green-500" aria-hidden="true" />
                Ready
              </span>
            </div>
            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
              Embeddings only, rule-based classification. No download needed.
            </p>
          </div>
        </button>

        {/* Tier 2: Built-in LLM */}
        <button
          type="button"
          onClick={() => setTier('builtin')}
          className={`w-full flex items-start gap-4 p-4 rounded-lg border-2 text-left transition-all ${
            tier === 'builtin'
              ? 'border-indigo-500 bg-indigo-50 dark:bg-indigo-900/20 dark:border-indigo-400'
              : 'border-gray-200 bg-white hover:border-gray-300 dark:bg-gray-800 dark:border-gray-700 dark:hover:border-gray-600'
          }`}
          aria-pressed={tier === 'builtin'}
        >
          <div className="shrink-0 mt-0.5">
            <span
              className="flex items-center justify-center w-8 h-8 rounded-full bg-purple-100 dark:bg-purple-900/30"
              aria-hidden="true"
            >
              <svg
                viewBox="0 0 20 20"
                fill="currentColor"
                className="w-5 h-5 text-purple-600 dark:text-purple-400"
              >
                <path
                  fillRule="evenodd"
                  d="M10 1a.75.75 0 01.75.75v1.5a.75.75 0 01-1.5 0v-1.5A.75.75 0 0110 1zM5.05 3.05a.75.75 0 011.06 0l1.062 1.06A.75.75 0 116.11 5.173L5.05 4.11a.75.75 0 010-1.06zm9.9 0a.75.75 0 010 1.06l-1.06 1.062a.75.75 0 01-1.062-1.061l1.061-1.06a.75.75 0 011.06 0zM10 7a3 3 0 100 6 3 3 0 000-6zm-6.25 3a.75.75 0 01-.75.75H1.5a.75.75 0 010-1.5H3a.75.75 0 01.75.75zm14.5 0a.75.75 0 01-.75.75h-1.5a.75.75 0 010-1.5H17a.75.75 0 01.75.75zm-11.09 4.828a.75.75 0 01-1.06 1.06L5.05 14.95a.75.75 0 011.06-1.06l1.06 1.06zm7.14-1.06a.75.75 0 011.06 0l1.06 1.06a.75.75 0 01-1.06 1.061l-1.06-1.06a.75.75 0 010-1.061zM10 17a.75.75 0 01.75.75v1.5a.75.75 0 01-1.5 0v-1.5A.75.75 0 0110 17z"
                  clipRule="evenodd"
                />
              </svg>
            </span>
          </div>
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                Local AI + Built-in LLM
              </span>
              <span className="inline-flex items-center rounded-full bg-green-100 px-2 py-0.5 text-[10px] font-medium text-green-700 dark:bg-green-900/30 dark:text-green-400">
                Recommended
              </span>
            </div>
            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
              Embeddings + local email classification. Downloads a small AI model (~350 MB).
            </p>
          </div>
        </button>

        {/* Tier 3: Ollama (Local LLM) */}
        <button
          type="button"
          onClick={() => setTier('ollama')}
          className={`w-full flex items-start gap-4 p-4 rounded-lg border-2 text-left transition-all ${
            tier === 'ollama'
              ? 'border-indigo-500 bg-indigo-50 dark:bg-indigo-900/20 dark:border-indigo-400'
              : 'border-gray-200 bg-white hover:border-gray-300 dark:bg-gray-800 dark:border-gray-700 dark:hover:border-gray-600'
          }`}
          aria-pressed={tier === 'ollama'}
        >
          <div className="shrink-0 mt-0.5">
            <span
              className={`flex items-center justify-center w-8 h-8 rounded-full ${
                ollamaStatus === 'connected'
                  ? 'bg-green-100 dark:bg-green-900/30'
                  : ollamaStatus === 'checking'
                    ? 'bg-yellow-100 dark:bg-yellow-900/30'
                    : 'bg-gray-100 dark:bg-gray-700'
              }`}
              aria-hidden="true"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth={1.5}
                className={`w-5 h-5 ${
                  ollamaStatus === 'connected'
                    ? 'text-green-600 dark:text-green-400'
                    : ollamaStatus === 'checking'
                      ? 'text-yellow-600 dark:text-yellow-400'
                      : 'text-gray-400 dark:text-gray-500'
                }`}
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M8.25 3v1.5M4.5 8.25H3m18 0h-1.5M4.5 12H3m18 0h-1.5m-15 3.75H3m18 0h-1.5M8.25 19.5V21M12 3v1.5m0 15V21m3.75-18v1.5m0 15V21m-9-1.5h10.5a2.25 2.25 0 002.25-2.25V6.75a2.25 2.25 0 00-2.25-2.25H6.75A2.25 2.25 0 004.5 6.75v10.5a2.25 2.25 0 002.25 2.25z"
                />
              </svg>
            </span>
          </div>
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                Local LLM (Ollama)
              </span>
              <span className="text-[10px] font-medium text-gray-500 dark:text-gray-400">
                Optional
              </span>
              {ollamaStatus === 'connected' && (
                <span className="inline-flex items-center gap-1 rounded-full bg-green-100 px-2 py-0.5 text-[10px] font-medium text-green-700 dark:bg-green-900/30 dark:text-green-400">
                  <span className="w-1.5 h-1.5 rounded-full bg-green-500" aria-hidden="true" />
                  Connected
                </span>
              )}
              {ollamaStatus === 'checking' && (
                <span className="inline-flex items-center gap-1 rounded-full bg-yellow-100 px-2 py-0.5 text-[10px] font-medium text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400">
                  <svg
                    className="animate-spin w-2.5 h-2.5"
                    viewBox="0 0 24 24"
                    fill="none"
                    aria-hidden="true"
                  >
                    <circle
                      className="opacity-25"
                      cx="12"
                      cy="12"
                      r="10"
                      stroke="currentColor"
                      strokeWidth="4"
                    />
                    <path
                      className="opacity-75"
                      fill="currentColor"
                      d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
                    />
                  </svg>
                  Checking...
                </span>
              )}
            </div>
            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
              {ollamaStatus === 'unavailable' ? (
                <>
                  Not detected — install{' '}
                  <a
                    href="https://ollama.com"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-indigo-600 hover:text-indigo-700 dark:text-indigo-400 dark:hover:text-indigo-300 underline"
                    onClick={(e) => e.stopPropagation()}
                  >
                    Ollama
                  </a>{' '}
                  for enhanced AI features like chat and advanced classification.
                </>
              ) : (
                'Run larger language models locally for chat, summarization, and advanced classification. Privacy-preserving.'
              )}
            </p>
          </div>
        </button>

        {/* Tier 4: Cloud AI */}
        <div>
          <button
            type="button"
            onClick={() => {
              setTier('cloud');
              setCloudExpanded(true);
            }}
            className={`w-full flex items-start gap-4 p-4 rounded-lg border-2 text-left transition-all ${
              tier === 'cloud'
                ? 'border-indigo-500 bg-indigo-50 dark:bg-indigo-900/20 dark:border-indigo-400'
                : 'border-gray-200 bg-white hover:border-gray-300 dark:bg-gray-800 dark:border-gray-700 dark:hover:border-gray-600'
            } ${tier === 'cloud' ? 'rounded-b-none' : ''}`}
            aria-pressed={tier === 'cloud'}
            aria-expanded={tier === 'cloud' && cloudExpanded}
          >
            <div className="shrink-0 mt-0.5">
              <span
                className="flex items-center justify-center w-8 h-8 rounded-full bg-blue-100 dark:bg-blue-900/30"
                aria-hidden="true"
              >
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth={1.5}
                  className="w-5 h-5 text-blue-600 dark:text-blue-400"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    d="M2.25 15a4.5 4.5 0 004.5 4.5H18a3.75 3.75 0 001.332-7.257 3 3 0 00-3.758-3.848 5.25 5.25 0 00-10.233 2.33A4.502 4.502 0 002.25 15z"
                  />
                </svg>
              </span>
            </div>
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2">
                <span className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                  Cloud AI
                </span>
                <span className="text-[10px] font-medium text-gray-500 dark:text-gray-400">
                  Optional
                </span>
              </div>
              <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                OpenAI, Anthropic, or Gemini for advanced features. Only needed for premium
                capabilities — all core features work with local AI.
              </p>
            </div>
          </button>

          {/* Cloud expanded section */}
          {tier === 'cloud' && cloudExpanded && (
            <div
              className="border-2 border-t-0 border-indigo-500 dark:border-indigo-400 rounded-b-lg
                bg-white dark:bg-gray-800 p-4 space-y-4"
            >
              <div>
                <label
                  htmlFor="cloud-provider"
                  className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1"
                >
                  Provider
                </label>
                <select
                  id="cloud-provider"
                  value={cloudProvider}
                  onChange={(e) => setCloudProvider(e.target.value as CloudProvider)}
                  className="w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm
                    focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
                    dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
                >
                  <option value="openai">OpenAI</option>
                  <option value="anthropic">Anthropic</option>
                  <option value="gemini">Google Gemini</option>
                </select>
              </div>
              <div>
                <label
                  htmlFor="cloud-api-key"
                  className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1"
                >
                  API Key
                </label>
                <div className="relative">
                  <input
                    id="cloud-api-key"
                    type={keyVisible ? 'text' : 'password'}
                    value={cloudApiKey}
                    onChange={(e) => setCloudApiKey(e.target.value)}
                    placeholder={
                      cloudProvider === 'openai'
                        ? 'sk-...'
                        : cloudProvider === 'anthropic'
                          ? 'sk-ant-...'
                          : 'AIza...'
                    }
                    autoComplete="off"
                    className="w-full rounded-lg border border-gray-300 bg-white px-3 py-2 pr-16 text-sm font-mono
                      focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
                      dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
                  />
                  <button
                    type="button"
                    onClick={() => setKeyVisible(!keyVisible)}
                    className="absolute right-2 top-1/2 -translate-y-1/2 px-2 py-0.5 text-xs text-gray-500
                      hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
                  >
                    {keyVisible ? 'Hide' : 'Show'}
                  </button>
                </div>
                <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Stored locally only — never sent to Emailibrium servers.
                </p>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Actions */}
      <div className="flex flex-col items-center gap-3 pt-2">
        <button
          type="button"
          onClick={handleContinue}
          className="w-full max-w-xs px-6 py-3 rounded-lg bg-indigo-600 text-white font-medium
            hover:bg-indigo-700 transition-colors"
        >
          Continue
        </button>
        <button
          type="button"
          onClick={onSkip}
          className="text-sm text-gray-500 hover:text-gray-700 dark:text-gray-400
            dark:hover:text-gray-200 transition-colors"
        >
          I&apos;ll configure this later in Settings
        </button>
      </div>
    </div>
  );
}
