/**
 * BL-3.05 -- useGenerativeRouter React hook
 *
 * Creates and manages a GenerativeRouter instance that stays in sync with
 * the user's settings store.
 */

import { useMemo, useEffect, useRef } from 'react';
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

  return useMemo(
    () => ({
      classify: router.classify.bind(router),
      chat: router.chat.bind(router),
      provider: config.provider,
      isReady: true,
    }),
    [router, config.provider],
  );
}
