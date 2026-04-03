import { type ReactNode, useEffect, useRef } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import type { AppStats } from './hooks/useStats';
import {
  getAccounts,
  getEmailCounts,
  getEmbeddingStatus,
  getIngestionProgress,
  getClusteringStatus,
  getBackfillProgress,
} from '@emailibrium/api';
import { useAppConfig } from '@/shared/hooks';

/** Map backend index type identifiers to human-friendly display names. */
function formatIndexType(raw?: string): string {
  if (!raw) return 'N/A';
  const map: Record<string, string> = {
    ruvector_hnsw: 'HNSW',
    hnsw: 'HNSW',
    flat: 'Flat',
    memory: 'In-Memory',
    sqlite: 'SQLite',
    qdrant: 'Qdrant',
  };
  return map[raw] ?? raw;
}

interface StatCardProps {
  icon: ReactNode;
  label: string;
  value: number | string;
  trend?: { direction: 'up' | 'down' | 'flat'; label: string };
}

function StatCard({ icon, label, value, trend }: StatCardProps) {
  return (
    <div className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm dark:border-gray-700 dark:bg-gray-800">
      <div className="flex items-center gap-3">
        <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-indigo-50 text-indigo-600 dark:bg-indigo-900/30 dark:text-indigo-400">
          {icon}
        </div>
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm text-gray-500 dark:text-gray-400">{label}</p>
          <p
            className="truncate text-2xl font-semibold text-gray-900 dark:text-white"
            title={String(value)}
          >
            {value}
          </p>
        </div>
      </div>
      {trend && (
        <div className="mt-3 flex items-center gap-1 text-xs">
          <span
            className={
              trend.direction === 'up'
                ? 'text-green-600 dark:text-green-400'
                : trend.direction === 'down'
                  ? 'text-red-600 dark:text-red-400'
                  : 'text-gray-500 dark:text-gray-400'
            }
          >
            {trend.direction === 'up' && '\u2191'}
            {trend.direction === 'down' && '\u2193'}
            {trend.direction === 'flat' && '\u2192'} {trend.label}
          </span>
        </div>
      )}
    </div>
  );
}

function StatCardSkeleton() {
  return (
    <div className="animate-pulse rounded-xl border border-gray-200 bg-white p-5 shadow-sm dark:border-gray-700 dark:bg-gray-800">
      <div className="flex items-center gap-3">
        <div className="h-10 w-10 rounded-lg bg-gray-200 dark:bg-gray-700" />
        <div className="flex-1 space-y-2">
          <div className="h-3 w-20 rounded bg-gray-200 dark:bg-gray-700" />
          <div className="h-6 w-16 rounded bg-gray-200 dark:bg-gray-700" />
        </div>
      </div>
    </div>
  );
}

interface StatsCardsProps {
  stats: AppStats | undefined;
  isLoading: boolean;
}

export function StatsCards({ stats, isLoading }: StatsCardsProps) {
  const { cache } = useAppConfig();
  const queryClient = useQueryClient();

  // Poll ingestion progress FIRST — drives adaptive intervals for other queries.
  const ingestionProgressQuery = useQuery({
    queryKey: ['dashboard-ingestion-progress'],
    queryFn: getIngestionProgress,
    staleTime: 2000,
    refetchInterval: 3000, // Fast poll — this is lightweight
  });

  const ingestionProgress = ingestionProgressQuery.data ?? null;
  const pipelineActive = ingestionProgress?.active ?? false;

  // Invalidate vector stats whenever ingestion progress changes during active pipeline.
  // This ensures the tiles update as emails are fetched, embedded, etc.
  const prevProcessedRef = useRef<number>(0);
  useEffect(() => {
    if (!pipelineActive) {
      prevProcessedRef.current = 0;
      return;
    }
    const processed = ingestionProgress?.processed ?? 0;
    const embedded = ingestionProgress?.embedded ?? 0;
    const current = processed + embedded;
    if (current !== prevProcessedRef.current) {
      prevProcessedRef.current = current;
      queryClient.invalidateQueries({ queryKey: ['stats'] });
      queryClient.invalidateQueries({ queryKey: ['dashboard-email-counts'] });
    }
  }, [pipelineActive, ingestionProgress?.processed, ingestionProgress?.embedded, queryClient]);

  const backfillQuery = useQuery({
    queryKey: ['dashboard-backfill-progress'],
    queryFn: getBackfillProgress,
    staleTime: 2000,
    refetchInterval: (query) => {
      const isActive = query.state.data?.active;
      return isActive || pipelineActive ? 3000 : 30000;
    },
  });

  // Invalidate categories-enriched-cc during and after backfill.
  const prevBackfillRef = useRef<number>(0);
  useEffect(() => {
    const backfillData = backfillQuery.data;
    if (!backfillData?.active) {
      if (prevBackfillRef.current > 0) {
        // Backfill just finished — final refresh
        queryClient.invalidateQueries({ queryKey: ['categories-enriched-cc'] });
        prevBackfillRef.current = 0;
      }
      return;
    }
    const current = backfillData.categorized + backfillData.failed;
    if (current !== prevBackfillRef.current) {
      prevBackfillRef.current = current;
      queryClient.invalidateQueries({ queryKey: ['categories-enriched-cc'] });
    }
  }, [backfillQuery.data, queryClient]);

  const accountsQuery = useQuery({
    queryKey: ['dashboard-accounts'],
    queryFn: getAccounts,
    staleTime: cache.dashboardAccountsRefetchIntervalMs / 2,
    refetchInterval: cache.dashboardAccountsRefetchIntervalMs,
  });

  const emailCountsQuery = useQuery({
    queryKey: ['dashboard-email-counts'],
    queryFn: getEmailCounts,
    staleTime: pipelineActive
      ? cache.ingestionActiveStaleTimeMs
      : cache.dashboardEmbeddingRefetchIntervalMs / 2,
    refetchInterval: pipelineActive
      ? cache.ingestionActiveRefetchIntervalMs
      : cache.dashboardEmbeddingRefetchIntervalMs,
  });

  const embeddingQuery = useQuery({
    queryKey: ['dashboard-embedding-status'],
    queryFn: getEmbeddingStatus,
    staleTime: cache.embeddingActiveRefetchIntervalMs / 2,
    // Poll fast (3s) whenever there are pending/stale emails or mutation is active.
    refetchInterval: (query) => {
      if (pipelineActive) return cache.embeddingActiveRefetchIntervalMs;
      const data = query.state.data;
      const hasPending =
        data &&
        (data.embeddingStatusSummary.pendingCount > 0 ||
          data.embeddingStatusSummary.staleCount > 0);
      return hasPending
        ? cache.embeddingActiveRefetchIntervalMs
        : cache.dashboardEmbeddingRefetchIntervalMs;
    },
  });

  // Derive "embedding in progress" from actual data OR pipeline phase.
  const embeddingInProgress =
    (embeddingQuery.data?.embeddingStatusSummary.pendingCount ?? 0) > 0 ||
    (embeddingQuery.data?.embeddingStatusSummary.staleCount ?? 0) > 0 ||
    (pipelineActive && ingestionProgress?.phase === 'embedding');

  const clusteringStatusQuery = useQuery({
    queryKey: ['dashboard-clustering-status'],
    queryFn: getClusteringStatus,
    staleTime: embeddingInProgress
      ? cache.embeddingActiveRefetchIntervalMs
      : cache.clusteringStatusStaleTimeMs,
    refetchInterval: embeddingInProgress
      ? cache.embeddingActiveRefetchIntervalMs
      : cache.clusteringStatusRefetchIntervalMs,
  });

  const accounts = accountsQuery.data ?? [];
  const accountCount = accounts.length;
  const accountsByProvider: Record<string, number> = {};
  for (const a of accounts) {
    accountsByProvider[a.provider] = (accountsByProvider[a.provider] || 0) + 1;
  }

  const emailCounts = emailCountsQuery.data ?? null;
  const embeddingStatus = embeddingQuery.data ?? null;
  const clusteringStatus = clusteringStatusQuery.data ?? null;

  if (isLoading) {
    return (
      <div
        className="grid grid-cols-2 gap-4 lg:grid-cols-4 xl:grid-cols-8"
        role="status"
        aria-label="Loading statistics"
      >
        {Array.from({ length: 8 }).map((_, i) => (
          <StatCardSkeleton key={i} />
        ))}
      </div>
    );
  }

  const cards: StatCardProps[] = [
    {
      icon: <AccountsIcon />,
      label: 'Accounts',
      value: accountCount.toString(),
    },
    {
      icon: <EmailCountIcon />,
      label: 'Emails',
      value: (emailCounts?.total ?? 0).toLocaleString(),
    },
    {
      icon: <UnreadIcon />,
      label: 'Unread',
      value: (emailCounts?.unread ?? 0).toLocaleString(),
    },
    {
      icon: <VectorIcon />,
      label: 'Total Vectors',
      value: stats?.totalVectors?.toLocaleString() ?? '0',
    },
    {
      icon: <InboxIcon />,
      label: 'Dimensions',
      value: stats?.dimensions?.toLocaleString() ?? '0',
    },
    {
      icon: <BellIcon />,
      label: 'Collections',
      value: stats?.collections ? Object.keys(stats.collections).length.toString() : '0',
    },
    {
      icon: <GridIcon />,
      label: 'Memory',
      value: stats?.memoryBytes ? `${(stats.memoryBytes / (1024 * 1024)).toFixed(1)} MB` : '0 MB',
    },
    {
      icon: <ZapIcon />,
      label: 'Index Type',
      value: formatIndexType(stats?.indexType),
    },
  ];

  const providerColors: Record<string, string> = {
    gmail: '#EA4335',
    outlook: '#0078D4',
    imap: '#6B7280',
    pop3: '#9CA3AF',
  };

  const totalAccounts = accountCount;

  return (
    <div className="space-y-4">
      <div
        className="grid grid-cols-2 gap-4 lg:grid-cols-4 xl:grid-cols-8"
        aria-label="Email statistics"
      >
        {cards.map((card) => (
          <StatCard key={card.label} {...card} />
        ))}

        {/* Accounts by Provider — spans 6 of 8 columns */}
        {totalAccounts > 0 && Object.keys(accountsByProvider).length > 0 && (
          <div className="col-span-2 rounded-xl border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-700 dark:bg-gray-800 lg:col-span-4 xl:col-span-6">
            <p className="text-sm font-medium text-gray-500 dark:text-gray-400 mb-3">
              Accounts by Provider
            </p>
            <div className="flex items-center gap-6">
              <svg viewBox="0 0 36 36" className="h-20 w-20 flex-shrink-0">
                {(() => {
                  const entries = Object.entries(accountsByProvider);
                  let offset = 0;
                  return entries.map(([provider, count]) => {
                    const pct = (count / totalAccounts) * 100;
                    const dashArray = `${pct} ${100 - pct}`;
                    const el = (
                      <circle
                        key={provider}
                        cx="18"
                        cy="18"
                        r="15.9155"
                        fill="none"
                        stroke={providerColors[provider] ?? '#6B7280'}
                        strokeWidth="3.5"
                        strokeDasharray={dashArray}
                        strokeDashoffset={-offset}
                        strokeLinecap="round"
                      />
                    );
                    offset += pct;
                    return el;
                  });
                })()}
                <text
                  x="18"
                  y="18"
                  textAnchor="middle"
                  dominantBaseline="central"
                  className="fill-gray-900 dark:fill-white"
                  fontSize="8"
                  fontWeight="600"
                >
                  {totalAccounts}
                </text>
              </svg>
              <div className="flex flex-wrap gap-x-6 gap-y-2">
                {Object.entries(accountsByProvider).map(([provider, count]) => (
                  <div key={provider} className="flex items-center gap-2">
                    <span
                      className="inline-block h-3 w-3 rounded-full"
                      style={{ backgroundColor: providerColors[provider] ?? '#6B7280' }}
                    />
                    <span className="text-sm capitalize text-gray-700 dark:text-gray-300">
                      {provider}
                    </span>
                    <span className="text-sm font-medium text-gray-900 dark:text-white">
                      {count}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        )}

        {/* AI Readiness — spans 2 of 8 columns; show during active pipeline even if no emails yet */}
        {(pipelineActive || (embeddingStatus && embeddingStatus.totalEmails > 0)) && (
          <div className="col-span-2 rounded-xl border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-700 dark:bg-gray-800">
            <p className="text-sm font-medium text-gray-500 dark:text-gray-400 mb-3">
              AI Readiness
            </p>
            {(() => {
              const { embeddedCount, pendingCount, failedCount } =
                embeddingStatus?.embeddingStatusSummary ?? {
                  embeddedCount: 0,
                  pendingCount: 0,
                  failedCount: 0,
                };
              const total = embeddingStatus?.totalEmails ?? 0;

              // Use pipeline progress for more accurate embedding % when pipeline is active.
              const pipelinePhase = pipelineActive ? ingestionProgress?.phase : null;
              const pipelineEmbedPct =
                pipelinePhase === 'embedding' && (ingestionProgress?.total ?? 0) > 0
                  ? Math.round(
                      ((ingestionProgress?.embedded ?? 0) / ingestionProgress!.total!) * 100,
                    )
                  : null;

              // Pipeline is past embedding (categorizing/clustering/analyzing/backfilling/complete) → treat as 100%
              const pastEmbedding =
                pipelineActive &&
                (pipelinePhase === 'categorizing' ||
                  pipelinePhase === 'clustering' ||
                  pipelinePhase === 'analyzing' ||
                  pipelinePhase === 'backfilling' ||
                  pipelinePhase === 'complete');

              const pct =
                pipelineEmbedPct !== null
                  ? pipelineEmbedPct
                  : pastEmbedding
                    ? 100
                    : total > 0
                      ? Math.round((embeddedCount / total) * 100)
                      : 0;

              const isReady = pct === 100 && !pipelineActive;
              const isInProgress =
                pendingCount > 0 || (pipelineActive && pipelinePhase !== 'complete');

              // Status label for embeddings
              let embeddingLabel: string;
              if (pipelinePhase === 'syncing') {
                embeddingLabel = `Fetching emails (${(ingestionProgress?.processed ?? 0).toLocaleString()})`;
              } else if (pipelinePhase === 'embedding') {
                embeddingLabel = `Embedding (${(ingestionProgress?.embedded ?? 0).toLocaleString()} / ${(ingestionProgress?.total ?? 0).toLocaleString()})`;
              } else if (pastEmbedding) {
                embeddingLabel = 'Embeddings complete';
              } else if (isReady) {
                embeddingLabel = 'Chat ready';
              } else if (pendingCount > 0) {
                embeddingLabel = `${pendingCount.toLocaleString()} pending`;
              } else {
                embeddingLabel = 'Waiting';
              }

              return (
                <div className="space-y-3">
                  <div>
                    <div className="flex items-center justify-between text-sm mb-1">
                      <span className="text-gray-700 dark:text-gray-300">Embeddings</span>
                      <span className="font-medium text-gray-900 dark:text-white">{pct}%</span>
                    </div>
                    <div className="h-2 w-full rounded-full bg-gray-200 dark:bg-gray-700">
                      <div
                        className={`h-2 rounded-full transition-all duration-500 ${
                          isReady
                            ? 'bg-green-500'
                            : isInProgress
                              ? 'bg-indigo-500'
                              : 'bg-yellow-500'
                        }`}
                        style={{ width: `${pct}%` }}
                      />
                    </div>
                  </div>
                  <div className="text-xs">
                    <span className="flex items-center gap-1.5">
                      <span
                        className={`inline-block h-2 w-2 rounded-full ${
                          isReady
                            ? 'bg-green-500'
                            : isInProgress
                              ? 'animate-pulse bg-indigo-500'
                              : 'bg-yellow-500'
                        }`}
                      />
                      <span className="text-gray-600 dark:text-gray-400">{embeddingLabel}</span>
                    </span>
                    {failedCount > 0 && (
                      <span className="flex items-center gap-1.5 mt-1">
                        <span className="inline-block h-2 w-2 rounded-full bg-red-500" />
                        <span className="text-red-600 dark:text-red-400">{failedCount} failed</span>
                      </span>
                    )}
                  </div>

                  {/* Clustering status */}
                  {(() => {
                    const clusteringActive =
                      clusteringStatus?.isClustering ||
                      (pipelineActive && pipelinePhase === 'clustering');
                    const clusterCount = clusteringStatus?.clusterCount ?? 0;
                    const isIngesting =
                      clusteringStatus?.isIngesting ||
                      (pipelineActive && pipelinePhase !== 'complete');

                    // Status label
                    let clusterLabel: string;
                    let clusterDetail: string;
                    let dotClass: string;

                    if (clusterCount > 0 && !clusteringActive && !isIngesting) {
                      clusterLabel = `${clusterCount} clusters`;
                      clusterDetail = `${(clusteringStatus?.totalClusteredEmails ?? 0).toLocaleString()} emails clustered`;
                      dotClass = 'bg-green-500';
                    } else if (clusteringActive) {
                      clusterLabel = 'Running...';
                      clusterDetail = 'Analyzing email topics...';
                      dotClass = 'animate-pulse bg-indigo-500';
                    } else if (
                      pipelineActive &&
                      (pipelinePhase === 'categorizing' ||
                        pipelinePhase === 'embedding' ||
                        pipelinePhase === 'analyzing')
                    ) {
                      clusterLabel = 'Queued';
                      clusterDetail =
                        pipelinePhase === 'embedding'
                          ? 'Will run after embedding'
                          : pipelinePhase === 'categorizing'
                            ? 'Will run after categorization'
                            : 'Finalizing analysis...';
                      dotClass = 'animate-pulse bg-yellow-500';
                    } else if (pipelinePhase === 'syncing') {
                      clusterLabel = 'Waiting';
                      clusterDetail = 'Waiting for emails to sync';
                      dotClass = 'animate-pulse bg-yellow-500';
                    } else if (pipelineActive && pipelinePhase === 'clustering') {
                      clusterLabel = 'Processing';
                      clusterDetail = 'Building topic clusters...';
                      dotClass = 'animate-pulse bg-indigo-500';
                    } else if (isReady) {
                      clusterLabel = 'Ready to run';
                      clusterDetail = 'Use "Run Clustering" below';
                      dotClass = 'bg-gray-400';
                    } else if (pipelineActive) {
                      clusterLabel = 'In progress';
                      clusterDetail = 'Pipeline active...';
                      dotClass = 'animate-pulse bg-yellow-500';
                    } else {
                      clusterLabel = '\u2014';
                      clusterDetail = 'Waiting for embeddings';
                      dotClass = 'bg-gray-400';
                    }

                    return (
                      <div className="border-t border-gray-100 pt-2 dark:border-gray-700">
                        <div className="flex items-center justify-between text-sm mb-1">
                          <span className="text-gray-700 dark:text-gray-300">Clustering</span>
                          <span className="font-medium text-gray-900 dark:text-white">
                            {clusterLabel}
                          </span>
                        </div>
                        <span className="flex items-center gap-1.5 text-xs">
                          <span className={`inline-block h-2 w-2 rounded-full ${dotClass}`} />
                          <span className="text-gray-600 dark:text-gray-400">{clusterDetail}</span>
                        </span>
                      </div>
                    );
                  })()}

                  {/* Categorization status — driven by backfill progress */}
                  {(() => {
                    const backfillData = backfillQuery.data;
                    if (!backfillData || backfillData.total === 0) return null;

                    if (backfillData.active) {
                      const categorizationPct =
                        backfillData.total > 0
                          ? Math.round((backfillData.categorized / backfillData.total) * 100)
                          : 0;
                      return (
                        <div className="border-t border-gray-100 pt-2 dark:border-gray-700">
                          <div className="flex items-center justify-between text-sm mb-1">
                            <span className="text-gray-700 dark:text-gray-300">Categorization</span>
                            <span className="font-medium text-gray-900 dark:text-white">
                              {categorizationPct}%
                            </span>
                          </div>
                          <div className="h-2 w-full rounded-full bg-gray-200 dark:bg-gray-700 mb-1.5">
                            <div
                              className="h-2 rounded-full bg-indigo-500 transition-all duration-500"
                              style={{ width: `${categorizationPct}%` }}
                            />
                          </div>
                          <span className="flex items-center gap-1.5 text-xs">
                            <span className="inline-block h-2 w-2 rounded-full animate-pulse bg-indigo-500" />
                            <span className="text-gray-600 dark:text-gray-400">
                              AI refining ({backfillData.categorized.toLocaleString()} /{' '}
                              {backfillData.total.toLocaleString()})
                            </span>
                          </span>
                        </div>
                      );
                    }

                    if (backfillData.categorized > 0) {
                      return (
                        <div className="border-t border-gray-100 pt-2 dark:border-gray-700">
                          <div className="flex items-center justify-between text-sm mb-1">
                            <span className="text-gray-700 dark:text-gray-300">Categorization</span>
                          </div>
                          <span className="flex items-center gap-1.5 text-xs">
                            <span className="inline-block h-2 w-2 rounded-full bg-green-500" />
                            <span className="text-gray-600 dark:text-gray-400">AI categorized</span>
                          </span>
                        </div>
                      );
                    }

                    return null;
                  })()}
                </div>
              );
            })()}
          </div>
        )}
      </div>
    </div>
  );
}

/* Inline SVG icon components to avoid external dependencies */

function UnreadIcon() {
  return (
    <svg
      className="h-5 w-5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M2.25 13.5h3.86a2.25 2.25 0 012.012 1.244l.256.512a2.25 2.25 0 002.013 1.244h3.218a2.25 2.25 0 002.013-1.244l.256-.512a2.25 2.25 0 012.013-1.244h3.859"
      />
      <circle cx="18" cy="5" r="3" fill="currentColor" />
    </svg>
  );
}

function VectorIcon() {
  return (
    <svg
      className="h-5 w-5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <circle cx="6" cy="6" r="2" />
      <circle cx="18" cy="8" r="2" />
      <circle cx="12" cy="18" r="2" />
      <circle cx="18" cy="18" r="2" />
      <path strokeLinecap="round" d="M7.5 7.5l3 8.5M16 9.5l-2.5 7M16.5 9l-9 8" />
    </svg>
  );
}

function EmailCountIcon() {
  return (
    <svg
      className="h-5 w-5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M21.75 6.75v10.5a2.25 2.25 0 01-2.25 2.25h-15a2.25 2.25 0 01-2.25-2.25V6.75m19.5 0A2.25 2.25 0 0019.5 4.5h-15a2.25 2.25 0 00-2.25 2.25m19.5 0v.243a2.25 2.25 0 01-1.07 1.916l-7.5 4.615a2.25 2.25 0 01-2.36 0L3.32 8.91a2.25 2.25 0 01-1.07-1.916V6.75"
      />
    </svg>
  );
}

function AccountsIcon() {
  return (
    <svg
      className="h-5 w-5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M15 19.128a9.38 9.38 0 002.625.372 9.337 9.337 0 004.121-.952 4.125 4.125 0 00-7.533-2.493M15 19.128v-.003c0-1.113-.285-2.16-.786-3.07M15 19.128v.106A12.318 12.318 0 018.624 21c-2.331 0-4.512-.645-6.374-1.766l-.001-.109a6.375 6.375 0 0111.964-3.07M12 6.375a3.375 3.375 0 11-6.75 0 3.375 3.375 0 016.75 0zm8.25 2.25a2.625 2.625 0 11-5.25 0 2.625 2.625 0 015.25 0z"
      />
    </svg>
  );
}

function InboxIcon() {
  return (
    <svg
      className="h-5 w-5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M20 13V6a2 2 0 00-2-2H6a2 2 0 00-2 2v7m16 0v5a2 2 0 01-2 2H6a2 2 0 01-2-2v-5m16 0h-2.586a1 1 0 00-.707.293l-2.414 2.414a1 1 0 01-.707.293h-3.172a1 1 0 01-.707-.293l-2.414-2.414A1 1 0 006.586 13H4"
      />
    </svg>
  );
}

function BellIcon() {
  return (
    <svg
      className="h-5 w-5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9"
      />
    </svg>
  );
}

function GridIcon() {
  return (
    <svg
      className="h-5 w-5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M4 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2V6zm10 0a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2V6zM4 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2v-2zm10 0a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2v-2z"
      />
    </svg>
  );
}

function ZapIcon() {
  return (
    <svg
      className="h-5 w-5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path strokeLinecap="round" strokeLinejoin="round" d="M13 10V3L4 14h7v7l9-11h-7z" />
    </svg>
  );
}
