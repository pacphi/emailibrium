import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import type { Cluster, ClusterTerm } from '@emailibrium/types';
import type { ClusteringStatus } from '@emailibrium/api';
import { getIngestionProgress } from '@emailibrium/api';

interface ClusterVisualizationProps {
  clusters?: Cluster[];
  isLoading?: boolean;
  clusteringStatus?: ClusteringStatus | null;
}

function ClusterCardSkeleton() {
  return (
    <div className="animate-pulse rounded-xl border border-gray-200 bg-white p-4 dark:border-gray-700 dark:bg-gray-800">
      <div className="mb-2 h-4 w-24 rounded bg-gray-200 dark:bg-gray-700" />
      <div className="mb-3 h-3 w-32 rounded bg-gray-200 dark:bg-gray-700" />
      <div className="h-2 w-full rounded-full bg-gray-200 dark:bg-gray-700" />
    </div>
  );
}

/** Map a TF-IDF score to a font size class (relative to the max score in this cluster). */
function termFontSize(score: number, maxScore: number): string {
  const ratio = maxScore > 0 ? score / maxScore : 0;
  if (ratio > 0.7) return 'text-base font-semibold';
  if (ratio > 0.4) return 'text-sm font-medium';
  if (ratio > 0.2) return 'text-xs font-medium';
  return 'text-[11px]';
}

/** Map a TF-IDF score to a color intensity. */
function termColor(score: number, maxScore: number): string {
  const ratio = maxScore > 0 ? score / maxScore : 0;
  if (ratio > 0.7) return 'text-indigo-700 dark:text-indigo-300';
  if (ratio > 0.4) return 'text-indigo-600 dark:text-indigo-400';
  if (ratio > 0.2) return 'text-gray-700 dark:text-gray-300';
  return 'text-gray-500 dark:text-gray-400';
}

function WordCloud({ terms }: { terms: ClusterTerm[] }) {
  if (!terms || terms.length === 0) return null;
  const maxScore = Math.max(...terms.map((t) => t.score));

  return (
    <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
      {terms.slice(0, 15).map((term) => (
        <span
          key={term.word}
          className={`cursor-default transition-opacity hover:opacity-80 ${termFontSize(term.score, maxScore)} ${termColor(term.score, maxScore)}`}
          title={`"${term.word}" appears in ${term.count} email${term.count !== 1 ? 's' : ''} (TF-IDF: ${term.score.toFixed(3)})`}
        >
          {term.word}
        </span>
      ))}
    </div>
  );
}

export function ClusterVisualization({
  clusters,
  isLoading,
  clusteringStatus,
}: ClusterVisualizationProps) {
  const [expandedId, setExpandedId] = useState<string | null>(null);

  // Poll ingestion progress to get real-time pipeline phase for accurate empty-state messaging.
  const ingestionProgressQuery = useQuery({
    queryKey: ['dashboard-ingestion-progress'],
    queryFn: getIngestionProgress,
    staleTime: 2000,
    refetchInterval: 3000,
  });
  const pipelineActive = ingestionProgressQuery.data?.active ?? false;
  const pipelinePhase = ingestionProgressQuery.data?.phase ?? null;

  if (isLoading) {
    return (
      <section aria-label="Topic clusters loading">
        <h2 className="mb-3 text-lg font-semibold text-gray-900 dark:text-white">Topic Clusters</h2>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 6 }).map((_, i) => (
            <ClusterCardSkeleton key={i} />
          ))}
        </div>
      </section>
    );
  }

  // Determine pipeline state — used for both empty and stale-cluster rendering.
  const isClustering = clusteringStatus?.isClustering ?? false;
  const isIngesting = clusteringStatus?.isIngesting ?? false;
  const clusterPhase = clusteringStatus?.phase;
  const effectivePhase = clusterPhase ?? pipelinePhase;
  const effectiveIngesting = isIngesting || pipelineActive;

  // During an active pipeline that hasn't reached clustering yet, suppress
  // stale clusters from a previous run and show progress instead.
  const preClustering =
    pipelineActive &&
    (pipelinePhase === 'syncing' ||
      pipelinePhase === 'embedding' ||
      pipelinePhase === 'categorizing' ||
      pipelinePhase === 'analyzing');
  const showProgressInsteadOfClusters = preClustering || !clusters || clusters.length === 0;

  if (showProgressInsteadOfClusters) {
    let message: string;
    if (isClustering || effectivePhase === 'clustering') {
      message = 'Clustering in progress — analyzing email topics...';
    } else if (effectivePhase === 'embedding') {
      message = 'Waiting for embeddings to complete before clustering...';
    } else if (effectivePhase === 'categorizing') {
      message = 'Categorizing emails — clustering will follow...';
    } else if (effectivePhase === 'analyzing') {
      message = 'Finalizing analysis — clusters will appear shortly...';
    } else if (effectivePhase === 'syncing') {
      message = 'Syncing emails — clustering will follow...';
    } else if (effectiveIngesting) {
      message = 'Email analysis in progress — clustering will follow...';
    } else {
      message = 'No clusters yet. Use "Full Sync" to rebuild embeddings and clusters.';
    }

    return (
      <section aria-label="Topic clusters">
        <h2 className="mb-3 text-lg font-semibold text-gray-900 dark:text-white">Topic Clusters</h2>
        <div className="rounded-xl border border-gray-200 bg-white p-4 dark:border-gray-700 dark:bg-gray-800">
          <div className="flex flex-col items-center gap-3 py-4">
            {isClustering || effectiveIngesting || effectivePhase === 'clustering' ? (
              <svg
                className="h-6 w-6 animate-spin text-indigo-500"
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
            ) : (
              <svg
                className="h-6 w-6 text-gray-400"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
                strokeWidth={1.5}
                aria-hidden="true"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M3.75 6A2.25 2.25 0 016 3.75h2.25A2.25 2.25 0 0110.5 6v2.25a2.25 2.25 0 01-2.25 2.25H6a2.25 2.25 0 01-2.25-2.25V6zM3.75 15.75A2.25 2.25 0 016 13.5h2.25a2.25 2.25 0 012.25 2.25V18a2.25 2.25 0 01-2.25 2.25H6A2.25 2.25 0 013.75 18v-2.25zM13.5 6a2.25 2.25 0 012.25-2.25H18A2.25 2.25 0 0120.25 6v2.25A2.25 2.25 0 0118 10.5h-2.25a2.25 2.25 0 01-2.25-2.25V6zM13.5 15.75a2.25 2.25 0 012.25-2.25H18a2.25 2.25 0 012.25 2.25V18A2.25 2.25 0 0118 20.25h-2.25A2.25 2.25 0 0113.5 18v-2.25z"
                />
              </svg>
            )}
            <p className="text-center text-sm text-gray-500 dark:text-gray-400">{message}</p>
          </div>
        </div>
      </section>
    );
  }

  const totalClustered = clusters.reduce((sum, c) => sum + c.emailCount, 0);

  return (
    <section aria-label="Topic clusters">
      <div className="mb-3 flex items-center gap-2">
        <h2 className="text-lg font-semibold text-gray-900 dark:text-white">Topic Clusters</h2>
        <span className="text-sm text-gray-500 dark:text-gray-400">
          {clusters.length} clusters, {totalClustered.toLocaleString()} emails
        </span>
      </div>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {clusters.map((cluster) => {
          const isExpanded = expandedId === cluster.id;
          const hasTerms = cluster.topTerms && cluster.topTerms.length > 0;
          const hasReps = cluster.representativeEmails && cluster.representativeEmails.length > 0;

          return (
            <div
              key={cluster.id}
              className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm transition-shadow hover:shadow-md dark:border-gray-700 dark:bg-gray-800"
            >
              {/* Header: email count + pin */}
              <div className="mb-2 flex items-center justify-between">
                <span className="text-xs font-medium text-gray-500 dark:text-gray-400">
                  {cluster.emailCount.toLocaleString()} emails
                </span>
                <div className="flex items-center gap-1.5">
                  {cluster.isPinned && (
                    <span className="text-xs text-indigo-500" title="Pinned">
                      <svg
                        className="h-3.5 w-3.5"
                        fill="currentColor"
                        viewBox="0 0 20 20"
                        aria-hidden="true"
                      >
                        <path d="M10.707 2.293a1 1 0 00-1.414 0l-7 7a1 1 0 001.414 1.414L4 10.414V17a1 1 0 001 1h2a1 1 0 001-1v-2a1 1 0 011-1h2a1 1 0 011 1v2a1 1 0 001 1h2a1 1 0 001-1v-6.586l.293.293a1 1 0 001.414-1.414l-7-7z" />
                      </svg>
                    </span>
                  )}
                  {hasReps && (
                    <button
                      type="button"
                      onClick={() => setExpandedId(isExpanded ? null : cluster.id)}
                      className="text-xs text-gray-400 hover:text-indigo-500 dark:hover:text-indigo-400"
                      aria-label={isExpanded ? 'Hide sample emails' : 'Show sample emails'}
                      title={isExpanded ? 'Hide samples' : 'Show sample emails'}
                    >
                      <svg
                        className={`h-4 w-4 transition-transform ${isExpanded ? 'rotate-180' : ''}`}
                        fill="none"
                        viewBox="0 0 24 24"
                        stroke="currentColor"
                        strokeWidth={2}
                      >
                        <path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" />
                      </svg>
                    </button>
                  )}
                </div>
              </div>

              {/* Word cloud */}
              {hasTerms ? (
                <div className="min-h-[3rem]">
                  <WordCloud terms={cluster.topTerms} />
                </div>
              ) : (
                <p className="text-sm font-medium text-gray-900 dark:text-white">{cluster.name}</p>
              )}

              {/* Representative emails (expandable) */}
              {isExpanded && hasReps && (
                <div className="mt-3 space-y-2 border-t border-gray-100 pt-3 dark:border-gray-700">
                  <p className="text-[10px] font-medium uppercase tracking-wider text-gray-400 dark:text-gray-500">
                    Representative emails
                  </p>
                  {cluster.representativeEmails.map((email) => (
                    <div
                      key={email.id}
                      className="rounded-lg bg-gray-50 px-3 py-2 dark:bg-gray-700/50"
                    >
                      <p className="truncate text-xs font-medium text-gray-800 dark:text-gray-200">
                        {email.subject || '(no subject)'}
                      </p>
                      <p className="truncate text-[11px] text-gray-500 dark:text-gray-400">
                        {email.fromName || email.fromAddr}
                      </p>
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}
