import { useQuery } from '@tanstack/react-query';
import { getHealth, getStats } from '@emailibrium/api';
import type { AppCacheConfig } from '@emailibrium/api';
import type { HealthStatus, VectorStats } from '@emailibrium/types';
import { useAppConfig } from '@/shared/hooks';

export type AppStats = VectorStats;

export interface StatsData {
  health: HealthStatus;
  stats: AppStats;
}

/**
 * TanStack Query hook that fetches both health status and aggregate stats.
 * The two queries run in parallel and results are combined.
 */
export function useHealthQuery() {
  return useQuery<HealthStatus>({
    queryKey: ['health'],
    queryFn: () => getHealth(),
    staleTime: 60_000,
    refetchInterval: 120_000,
  });
}

export function useStatsQuery(cache: AppCacheConfig, isActive = false) {
  return useQuery<VectorStats>({
    queryKey: ['stats'],
    queryFn: () => getStats(),
    staleTime: isActive ? cache.ingestionActiveStaleTimeMs : cache.statsRefetchIntervalMs / 3,
    refetchInterval: isActive ? cache.statsActiveRefetchIntervalMs : cache.statsRefetchIntervalMs,
  });
}

export function useStats(isActive = false) {
  const { cache } = useAppConfig();
  const health = useHealthQuery();
  const stats = useStatsQuery(cache, isActive);

  // Use the most recent dataUpdatedAt from either query as "last updated".
  const dataUpdatedAt = Math.max(stats.dataUpdatedAt ?? 0, health.dataUpdatedAt ?? 0);

  return {
    health: health.data,
    stats: stats.data,
    isLoading: health.isLoading || stats.isLoading,
    isError: health.isError || stats.isError,
    error: health.error ?? stats.error,
    dataUpdatedAt: dataUpdatedAt > 0 ? dataUpdatedAt : undefined,
    refetch: () => {
      health.refetch();
      stats.refetch();
    },
  };
}
