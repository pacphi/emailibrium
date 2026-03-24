import { useState } from 'react';
import type { Cluster } from '@emailibrium/types';
import type { ClusterAction } from './hooks/useInboxCleaner';

interface Step3TopicsProps {
  clusters: Cluster[];
  clusterSelections: Map<string, ClusterAction>;
  onSetAction: (clusterId: string, action: ClusterAction) => void;
  isLoading: boolean;
}

const actionOptions: Array<{ value: ClusterAction; label: string }> = [
  { value: 'keep', label: 'Keep' },
  { value: 'archive-old', label: 'Archive Old' },
  { value: 'archive-all', label: 'Archive All' },
  { value: 'delete-all', label: 'Delete All' },
  { value: 'review', label: 'Review Later' },
];

const actionBadgeColors: Record<ClusterAction, string> = {
  keep: 'bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300',
  'archive-old': 'bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300',
  'archive-all': 'bg-orange-100 text-orange-700 dark:bg-orange-900/40 dark:text-orange-300',
  'delete-all': 'bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300',
  review: 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300',
};

interface ClusterCardProps {
  cluster: Cluster;
  action: ClusterAction;
  onSetAction: (action: ClusterAction) => void;
}

function ClusterCard({ cluster, action, onSetAction }: ClusterCardProps) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 overflow-hidden">
      <div className="px-4 py-3 flex items-center gap-3">
        {/* Cluster info */}
        <button
          onClick={() => setExpanded(!expanded)}
          className="flex-1 text-left min-w-0"
          aria-expanded={expanded}
        >
          <div className="flex items-center gap-2">
            <h4 className="text-sm font-semibold text-gray-900 dark:text-gray-100 truncate">
              {cluster.name}
            </h4>
            <span className="inline-flex px-2 py-0.5 text-xs font-medium rounded-full bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-300">
              {cluster.emailCount.toLocaleString()} emails
            </span>
            {cluster.stabilityScore < 0.5 && (
              <span className="inline-flex px-2 py-0.5 text-[10px] font-medium rounded-full bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300">
                Cross-account
              </span>
            )}
          </div>
          <p className="mt-0.5 text-xs text-gray-500 dark:text-gray-400 truncate">
            {cluster.description}
          </p>
        </button>

        {/* Action dropdown */}
        <div className="shrink-0">
          <select
            value={action}
            onChange={(e) => onSetAction(e.target.value as ClusterAction)}
            className={`text-xs font-medium rounded-md border-0 py-1.5 px-3 cursor-pointer focus:ring-2 focus:ring-blue-500 ${actionBadgeColors[action]}`}
          >
            {actionOptions.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </div>

        {/* Expand toggle */}
        <button
          onClick={() => setExpanded(!expanded)}
          className="shrink-0 p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
          aria-label={expanded ? 'Collapse' : 'Expand'}
        >
          <svg
            className={`w-4 h-4 transition-transform ${expanded ? 'rotate-180' : ''}`}
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
          </svg>
        </button>
      </div>

      {/* Expanded preview */}
      {expanded && (
        <div className="border-t border-gray-100 dark:border-gray-700 px-4 py-3 bg-gray-50 dark:bg-gray-800/50">
          <p className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-2">
            Sample subjects:
          </p>
          <ul className="space-y-1">
            {/* Placeholder samples derived from cluster description */}
            <li className="text-xs text-gray-600 dark:text-gray-300 truncate">
              &bull; {cluster.description} - Example email 1
            </li>
            <li className="text-xs text-gray-600 dark:text-gray-300 truncate">
              &bull; {cluster.description} - Example email 2
            </li>
            <li className="text-xs text-gray-600 dark:text-gray-300 truncate">
              &bull; {cluster.description} - Example email 3
            </li>
          </ul>
          <p className="mt-2 text-[10px] text-gray-400">
            Stability: {(cluster.stabilityScore * 100).toFixed(0)}% | Created:{' '}
            {new Date(cluster.createdAt).toLocaleDateString()}
          </p>
        </div>
      )}
    </div>
  );
}

export function Step3Topics({
  clusters,
  clusterSelections,
  onSetAction,
  isLoading,
}: Step3TopicsProps) {
  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <div className="flex flex-col items-center gap-3">
          <div className="w-8 h-8 border-4 border-blue-500 border-t-transparent rounded-full animate-spin" />
          <p className="text-sm text-gray-500 dark:text-gray-400">Loading topic clusters...</p>
        </div>
      </div>
    );
  }

  if (clusters.length === 0) {
    return (
      <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-8 text-center">
        <p className="text-sm text-gray-500 dark:text-gray-400">
          No topic clusters found. The analysis may still be in progress.
        </p>
      </div>
    );
  }

  const totalEmailsAffected = clusters.reduce((sum, c) => {
    const action = clusterSelections.get(c.id) ?? 'keep';
    return action !== 'keep' && action !== 'review' ? sum + c.emailCount : sum;
  }, 0);

  return (
    <div className="space-y-4">
      {/* Summary */}
      {totalEmailsAffected > 0 && (
        <div className="rounded-lg bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800 px-4 py-3">
          <p className="text-sm text-amber-700 dark:text-amber-300">
            <span className="font-semibold">{totalEmailsAffected.toLocaleString()}</span> emails
            across{' '}
            <span className="font-semibold">
              {
                clusters.filter((c) => {
                  const a = clusterSelections.get(c.id) ?? 'keep';
                  return a !== 'keep' && a !== 'review';
                }).length
              }
            </span>{' '}
            clusters will be processed.
          </p>
        </div>
      )}

      {/* Cluster list */}
      <div className="space-y-3">
        {clusters.map((cluster) => (
          <ClusterCard
            key={cluster.id}
            cluster={cluster}
            action={clusterSelections.get(cluster.id) ?? 'keep'}
            onSetAction={(action) => onSetAction(cluster.id, action)}
          />
        ))}
      </div>
    </div>
  );
}
