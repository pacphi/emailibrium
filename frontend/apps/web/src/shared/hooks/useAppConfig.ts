import { useQuery } from '@tanstack/react-query';
import type { AppConfig } from '@emailibrium/api';
import { getAppConfig } from '@emailibrium/api';

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
 */
export function useAppConfig(): AppConfig {
  const { data } = useQuery({
    queryKey: ['app-config'],
    queryFn: getAppConfig,
    staleTime: Infinity, // Config doesn't change at runtime
    gcTime: Infinity,
  });

  return data ?? DEFAULTS;
}
