import type { Cluster } from '@emailibrium/types';

interface ClusterVisualizationProps {
  clusters?: Cluster[];
  isLoading?: boolean;
}

/** Mock cluster data used when the API has not returned real data yet. */
const MOCK_CLUSTERS: Cluster[] = [
  {
    id: '1',
    name: 'Work Projects',
    description: 'Project updates and tasks',
    emailCount: 234,
    stabilityScore: 0.92,
    isPinned: true,
    createdAt: '2026-03-01',
  },
  {
    id: '2',
    name: 'Newsletters',
    description: 'Subscribed newsletters',
    emailCount: 189,
    stabilityScore: 0.88,
    isPinned: false,
    createdAt: '2026-03-05',
  },
  {
    id: '3',
    name: 'Finance',
    description: 'Banking and transactions',
    emailCount: 145,
    stabilityScore: 0.95,
    isPinned: true,
    createdAt: '2026-02-20',
  },
  {
    id: '4',
    name: 'Social',
    description: 'Social media notifications',
    emailCount: 112,
    stabilityScore: 0.78,
    isPinned: false,
    createdAt: '2026-03-10',
  },
  {
    id: '5',
    name: 'Shopping',
    description: 'Orders and receipts',
    emailCount: 87,
    stabilityScore: 0.85,
    isPinned: false,
    createdAt: '2026-03-12',
  },
  {
    id: '6',
    name: 'Travel',
    description: 'Bookings and itineraries',
    emailCount: 43,
    stabilityScore: 0.91,
    isPinned: false,
    createdAt: '2026-03-15',
  },
];

function ClusterCardSkeleton() {
  return (
    <div className="animate-pulse rounded-xl border border-gray-200 bg-white p-4 dark:border-gray-700 dark:bg-gray-800">
      <div className="mb-2 h-4 w-24 rounded bg-gray-200 dark:bg-gray-700" />
      <div className="mb-3 h-3 w-32 rounded bg-gray-200 dark:bg-gray-700" />
      <div className="h-2 w-full rounded-full bg-gray-200 dark:bg-gray-700" />
    </div>
  );
}

export function ClusterVisualization({ clusters, isLoading }: ClusterVisualizationProps) {
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

  const data = clusters ?? MOCK_CLUSTERS;
  const maxCount = Math.max(...data.map((c) => c.emailCount), 1);

  return (
    <section aria-label="Topic clusters">
      <h2 className="mb-3 text-lg font-semibold text-gray-900 dark:text-white">Topic Clusters</h2>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {data.map((cluster) => {
          const widthPercent = Math.round((cluster.emailCount / maxCount) * 100);
          return (
            <div
              key={cluster.id}
              className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm transition-shadow hover:shadow-md dark:border-gray-700 dark:bg-gray-800"
            >
              <div className="mb-1 flex items-center justify-between">
                <h3 className="text-sm font-semibold text-gray-900 dark:text-white">
                  {cluster.name}
                </h3>
                {cluster.isPinned && (
                  <span
                    className="text-xs text-indigo-500"
                    aria-label="Pinned cluster"
                    title="Pinned"
                  >
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
              </div>
              <p className="mb-3 text-xs text-gray-500 dark:text-gray-400">
                {cluster.emailCount.toLocaleString()} emails
              </p>
              <div
                className="h-2 overflow-hidden rounded-full bg-gray-100 dark:bg-gray-700"
                role="meter"
                aria-label={`${cluster.name}: ${cluster.emailCount} emails`}
                aria-valuenow={cluster.emailCount}
                aria-valuemin={0}
                aria-valuemax={maxCount}
              >
                <div
                  className="h-full rounded-full bg-indigo-500 transition-all dark:bg-indigo-400"
                  style={{ width: `${widthPercent}%` }}
                />
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}
