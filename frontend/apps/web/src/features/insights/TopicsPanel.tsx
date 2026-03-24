import { useState } from 'react';
import type { SubscriptionInsight } from '@emailibrium/types';

interface TopicCluster {
  id: string;
  name: string;
  emailCount: number;
  dateRange: { start: string; end: string };
  topSenders: string[];
  sampleSubjects: string[];
  isPinned: boolean;
}

interface TopicsPanelProps {
  senders: SubscriptionInsight[] | undefined;
  isLoading: boolean;
}

function buildClusters(senders: SubscriptionInsight[]): TopicCluster[] {
  const categoryMap = new Map<string, SubscriptionInsight[]>();
  for (const s of senders) {
    const key = s.category || 'unknown';
    const list = categoryMap.get(key) ?? [];
    list.push(s);
    categoryMap.set(key, list);
  }

  return Array.from(categoryMap.entries()).map(([category, items]) => {
    const sorted = [...items].sort(
      (a, b) => new Date(a.firstSeen).getTime() - new Date(b.firstSeen).getTime(),
    );
    return {
      id: category,
      name: category.charAt(0).toUpperCase() + category.slice(1),
      emailCount: items.reduce((sum, i) => sum + i.emailCount, 0),
      dateRange: {
        start: sorted[0]?.firstSeen ?? '',
        end: sorted[sorted.length - 1]?.lastSeen ?? '',
      },
      topSenders: items
        .sort((a, b) => b.emailCount - a.emailCount)
        .slice(0, 3)
        .map((i) => i.senderAddress),
      sampleSubjects: items.slice(0, 3).map((i) => `${i.senderDomain} updates`),
      isPinned: false,
    };
  });
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
  onTogglePin,
  onClick,
}: {
  cluster: TopicCluster;
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
          </p>
        </div>
        <button
          onClick={(e) => {
            e.stopPropagation();
            onTogglePin();
          }}
          className="rounded p-1 hover:bg-gray-100 dark:hover:bg-gray-700"
          aria-label={cluster.isPinned ? 'Unpin cluster' : 'Pin cluster'}
        >
          <PinIcon pinned={cluster.isPinned} />
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
        <p className="mb-1 text-xs font-medium text-gray-500 dark:text-gray-400">Sample Subjects</p>
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

export function TopicsPanel({ senders, isLoading }: TopicsPanelProps) {
  const [clusters, setClusters] = useState<TopicCluster[]>([]);
  const [initialized, setInitialized] = useState(false);

  if (!initialized && senders && senders.length > 0) {
    setClusters(buildClusters(senders));
    setInitialized(true);
  }

  if (isLoading) return <PanelSkeleton />;

  const pinned = clusters.filter((c) => c.isPinned);
  const unpinned = clusters.filter((c) => !c.isPinned);
  const sorted = [...pinned, ...unpinned];

  const togglePin = (id: string) => {
    setClusters((prev) => prev.map((c) => (c.id === id ? { ...c, isPinned: !c.isPinned } : c)));
  };

  if (sorted.length === 0) {
    return (
      <p className="py-12 text-center text-sm text-gray-400">
        No topic clusters available. Clusters are generated from email analysis.
      </p>
    );
  }

  return (
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {sorted.map((cluster) => (
        <ClusterCard
          key={cluster.id}
          cluster={cluster}
          onTogglePin={() => togglePin(cluster.id)}
          onClick={() => {
            /* TODO: navigate to cluster detail */
          }}
        />
      ))}
    </div>
  );
}
