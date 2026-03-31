import { useState } from 'react';
import type { TopicCluster } from '@emailibrium/api';
import { useTopicClusters } from './hooks/useInsights';

interface TopicsPanelProps {
  senders?: unknown;
  isLoading?: boolean;
}

function PinIcon({ pinned }: { pinned: boolean }) {
  return (
    <svg
      className={`h-4 w-4 ${pinned ? 'text-indigo-600 dark:text-indigo-400' : 'text-gray-400 hover:text-gray-600 dark:hover:text-gray-300'}`}
      fill={pinned ? 'currentColor' : 'none'}
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M5 5a2 2 0 012-2h10a2 2 0 012 2v16l-7-3.5L5 21V5z"
      />
    </svg>
  );
}

function ClusterCard({
  cluster,
  isPinned,
  onTogglePin,
  onClick,
}: {
  cluster: TopicCluster;
  isPinned: boolean;
  onTogglePin: () => void;
  onClick: () => void;
}) {
  const startDate = cluster.dateRange.start
    ? new Date(cluster.dateRange.start).toLocaleDateString()
    : 'N/A';
  const endDate = cluster.dateRange.end
    ? new Date(cluster.dateRange.end).toLocaleDateString()
    : 'N/A';

  return (
    <div
      className="group cursor-pointer rounded-xl border border-gray-200 bg-white p-5 shadow-sm transition-shadow hover:shadow-md dark:border-gray-700 dark:bg-gray-800"
      onClick={onClick}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') onClick();
      }}
    >
      <div className="mb-3 flex items-start justify-between">
        <div>
          <h4 className="text-sm font-semibold text-gray-900 dark:text-white">{cluster.name}</h4>
          <p className="text-xs text-gray-500 dark:text-gray-400">
            {cluster.emailCount.toLocaleString()} emails
            {cluster.unreadCount > 0 && (
              <span className="ml-1 text-indigo-600 dark:text-indigo-400">
                ({cluster.unreadCount.toLocaleString()} unread)
              </span>
            )}
          </p>
        </div>
        <button
          onClick={(e) => {
            e.stopPropagation();
            onTogglePin();
          }}
          className="rounded p-1 hover:bg-gray-100 dark:hover:bg-gray-700"
          aria-label={isPinned ? 'Unpin cluster' : 'Pin cluster'}
        >
          <PinIcon pinned={isPinned} />
        </button>
      </div>

      <p className="mb-3 text-xs text-gray-400 dark:text-gray-500">
        {startDate} - {endDate}
      </p>

      <div className="mb-3">
        <p className="mb-1 text-xs font-medium text-gray-500 dark:text-gray-400">Top Senders</p>
        <div className="space-y-0.5">
          {cluster.topSenders.map((sender) => (
            <p key={sender} className="truncate text-xs text-gray-600 dark:text-gray-300">
              {sender}
            </p>
          ))}
        </div>
      </div>

      <div>
        <p className="mb-1 text-xs font-medium text-gray-500 dark:text-gray-400">Recent Subjects</p>
        <div className="space-y-0.5">
          {cluster.sampleSubjects.map((subject, i) => (
            <p key={i} className="truncate text-xs text-gray-500 italic dark:text-gray-400">
              {subject}
            </p>
          ))}
        </div>
      </div>
    </div>
  );
}

function PanelSkeleton() {
  return (
    <div className="animate-pulse grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {Array.from({ length: 6 }).map((_, i) => (
        <div key={i} className="h-48 rounded-xl bg-gray-200 dark:bg-gray-700" />
      ))}
    </div>
  );
}

export function TopicsPanel(_props: TopicsPanelProps) {
  const { data: clusters, isLoading } = useTopicClusters();
  const [pinnedIds, setPinnedIds] = useState<Set<string>>(new Set());

  const togglePin = (id: string) => {
    setPinnedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  if (isLoading) return <PanelSkeleton />;

  if (!clusters || clusters.length === 0) {
    return (
      <p className="py-12 text-center text-sm text-gray-400">
        No topic clusters available. Clusters are generated from email analysis.
      </p>
    );
  }

  const sorted = [...clusters].sort((a, b) => {
    const aPinned = pinnedIds.has(a.id) ? 0 : 1;
    const bPinned = pinnedIds.has(b.id) ? 0 : 1;
    if (aPinned !== bPinned) return aPinned - bPinned;
    return b.emailCount - a.emailCount;
  });

  return (
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {sorted.map((cluster) => (
        <ClusterCard
          key={cluster.id}
          cluster={cluster}
          isPinned={pinnedIds.has(cluster.id)}
          onTogglePin={() => togglePin(cluster.id)}
          onClick={() => {
            // Navigate to email view with this category pre-selected in the sidebar.
            window.location.href = `/email?group=${encodeURIComponent(cluster.id)}`;
          }}
        />
      ))}
    </div>
  );
}
