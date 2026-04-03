import { useQuery } from '@tanstack/react-query';
import type { AppConfig } from '@emailibrium/api';
import { getAppConfig } from '@emailibrium/api';

/** Convert snake_case keys to camelCase (one level deep). */
function snakeToCamel(obj: Record<string, unknown>): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(obj)) {
    const camelKey = key.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
    result[camelKey] = value;
  }
  return result;
}

/** Default config values used before the server responds. */
const DEFAULTS: AppConfig = {
  cache: {
    defaultStaleTimeMs: 30_000,
    defaultRetryCount: 1,
    clustersStaleTimeMs: 10_000,
    clustersRefetchIntervalMs: 30_000,
    clustersActiveStaleTimeMs: 3_000,
    clustersActiveRefetchIntervalMs: 5_000,
    clusteringStatusStaleTimeMs: 5_000,
    clusteringStatusRefetchIntervalMs: 10_000,
    dashboardAccountsRefetchIntervalMs: 10_000,
    dashboardEmbeddingRefetchIntervalMs: 10_000,
    embeddingActiveRefetchIntervalMs: 5_000,
    ingestionActiveRefetchIntervalMs: 3_000,
    ingestionActiveStaleTimeMs: 2_000,
    statsRefetchIntervalMs: 30_000,
    statsActiveRefetchIntervalMs: 5_000,
  },
  network: {
    ingestionStartTimeoutMs: 300_000,
    reclusterTimeoutMs: 300_000,
    reembedTimeoutMs: 60_000,
  },
};

/**
 * Fetch and cache the server-side app.yaml configuration.
 * The config is fetched once and cached for the session lifetime.
 *
 * The backend YAML uses snake_case keys, but the frontend expects camelCase.
 * We transform on receipt and deep-merge with DEFAULTS so every key has a
 * guaranteed fallback value.
 */
export function useAppConfig(): AppConfig {
  const { data } = useQuery({
    queryKey: ['app-config'],
    queryFn: getAppConfig,
    staleTime: Infinity, // Config doesn't change at runtime
    gcTime: Infinity,
  });

  if (!data) return DEFAULTS;

  // Deep-merge: transform snake_case server keys → camelCase, then overlay on defaults.
  const remoteCache = data.cache
    ? snakeToCamel(data.cache as unknown as Record<string, unknown>)
    : {};
  const remoteNetwork = data.network
    ? snakeToCamel(data.network as unknown as Record<string, unknown>)
    : {};

  return {
    cache: { ...DEFAULTS.cache, ...remoteCache },
    network: { ...DEFAULTS.network, ...remoteNetwork },
  } as AppConfig;
}
