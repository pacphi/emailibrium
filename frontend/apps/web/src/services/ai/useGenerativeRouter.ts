/**
 * BL-3.05 -- useGenerativeRouter React hook
 *
 * Creates and manages a GenerativeRouter instance that stays in sync with
 * the user's settings store.
 */

import { useMemo, useEffect, useRef, useState } from 'react';
import { useSettings } from '../../features/settings/hooks/useSettings';
import { GenerativeRouter } from './generative-router';
import type { GenerativeProvider, GenerativeRouterConfig } from './generative-router';

export function useGenerativeRouter() {
  const llmProvider = useSettings((s) => s.llmProvider);
  const builtInLlmModel = useSettings((s) => s.builtInLlmModel);
  const ollamaBaseUrl = useSettings((s) => s.ollamaBaseUrl);
  const openaiApiKey = useSettings((s) => s.openaiApiKey);
  const anthropicApiKey = useSettings((s) => s.anthropicApiKey);

  const config: GenerativeRouterConfig = useMemo(
    () => ({
      provider: llmProvider as GenerativeProvider,
      builtInModelId: builtInLlmModel,
      ollamaBaseUrl,
      openaiApiKey,
      anthropicApiKey,
    }),
    [llmProvider, builtInLlmModel, ollamaBaseUrl, openaiApiKey, anthropicApiKey],
  );

  const routerRef = useRef<GenerativeRouter | null>(null);

  if (!routerRef.current) {
    routerRef.current = new GenerativeRouter(config);
  }

  // Keep router config in sync with settings changes
  useEffect(() => {
    routerRef.current?.updateConfig(config);
  }, [config]);

  // Clean up on unmount
  useEffect(() => {
    return () => {
      void routerRef.current?.shutdown();
    };
  }, []);

  const router = routerRef.current;

  const activeModel = useMemo(() => {
    switch (config.provider) {
      case 'builtin':
        return config.builtInModelId || 'qwen3-1.7b-q4km';
      case 'local':
        return 'Ollama';
      case 'openai':
        return 'OpenAI';
      case 'anthropic':
        return 'Anthropic';
      default:
        return 'Rule-based';
    }
  }, [config.provider, config.builtInModelId]);

  // Resolve tool calling capability for the active model.
  // Cloud providers always support tool calling. For builtin, check the model catalog.
  const [toolCalling, setToolCalling] = useState(config.provider !== 'none');

  useEffect(() => {
    if (config.provider === 'openai' || config.provider === 'anthropic') {
      setToolCalling(true);
      return;
    }
    if (config.provider === 'none') {
      setToolCalling(false);
      return;
    }
    if (config.provider === 'builtin') {
      fetch('/api/v1/ai/model-catalog', { signal: AbortSignal.timeout(3000) })
        .then((r) => (r.ok ? r.json() : []))
        .then((models: { id: string; toolCalling?: boolean }[]) => {
          const match = models.find((m) => m.id === config.builtInModelId);
          setToolCalling(match?.toolCalling ?? false);
        })
        .catch(() => setToolCalling(false));
      return;
    }
    // Ollama: assume true by default (most models on Ollama support tool calling)
    setToolCalling(true);
  }, [config.provider, config.builtInModelId]);

  return useMemo(
    () => ({
      classify: router.classify.bind(router),
      chat: router.chat.bind(router),
      provider: config.provider,
      activeModel,
      toolCalling,
      isReady: true,
    }),
    [router, config.provider, activeModel, toolCalling],
  );
}
