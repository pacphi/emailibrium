import { api } from './client.js';

export interface AppCacheConfig {
  defaultStaleTimeMs: number;
  defaultRetryCount: number;
  clustersStaleTimeMs: number;
  clustersRefetchIntervalMs: number;
  clustersActiveStaleTimeMs: number;
  clustersActiveRefetchIntervalMs: number;
  clusteringStatusStaleTimeMs: number;
  clusteringStatusRefetchIntervalMs: number;
  dashboardAccountsRefetchIntervalMs: number;
  dashboardEmbeddingRefetchIntervalMs: number;
  embeddingActiveRefetchIntervalMs: number;
}

export interface AppNetworkConfig {
  ingestionStartTimeoutMs: number;
  reclusterTimeoutMs: number;
  reembedTimeoutMs: number;
}

export interface AppConfig {
  cache: AppCacheConfig;
  network: AppNetworkConfig;
}

export async function getAppConfig(): Promise<AppConfig> {
  return api.get('ai/config/app').json<AppConfig>();
}
