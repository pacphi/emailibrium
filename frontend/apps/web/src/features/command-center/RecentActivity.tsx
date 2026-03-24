interface ActivityEvent {
  id: string;
  type: 'sync' | 'rule' | 'cluster' | 'subscription' | 'insight';
  message: string;
  timestamp: string;
}

/** Mock recent activity data for initial rendering. */
const MOCK_ACTIVITY: ActivityEvent[] = [
  {
    id: '1',
    type: 'sync',
    message: 'Synced 47 new emails from Gmail',
    timestamp: '2 minutes ago',
  },
  {
    id: '2',
    type: 'rule',
    message: 'Rule "Archive Newsletters" processed 12 emails',
    timestamp: '15 minutes ago',
  },
  {
    id: '3',
    type: 'cluster',
    message: 'New topic cluster detected: "Project Updates"',
    timestamp: '1 hour ago',
  },
  {
    id: '4',
    type: 'subscription',
    message: '3 new subscriptions identified',
    timestamp: '2 hours ago',
  },
  {
    id: '5',
    type: 'insight',
    message: 'Weekly email volume down 15% from last week',
    timestamp: '5 hours ago',
  },
  {
    id: '6',
    type: 'sync',
    message: 'Synced 23 new emails from Outlook',
    timestamp: '6 hours ago',
  },
  {
    id: '7',
    type: 'rule',
    message: 'Rule "Star Important" flagged 5 emails',
    timestamp: '8 hours ago',
  },
];

const TYPE_STYLES: Record<ActivityEvent['type'], { bg: string; text: string; label: string }> = {
  sync: {
    bg: 'bg-blue-100 dark:bg-blue-900/30',
    text: 'text-blue-700 dark:text-blue-300',
    label: 'Sync',
  },
  rule: {
    bg: 'bg-purple-100 dark:bg-purple-900/30',
    text: 'text-purple-700 dark:text-purple-300',
    label: 'Rule',
  },
  cluster: {
    bg: 'bg-green-100 dark:bg-green-900/30',
    text: 'text-green-700 dark:text-green-300',
    label: 'Cluster',
  },
  subscription: {
    bg: 'bg-orange-100 dark:bg-orange-900/30',
    text: 'text-orange-700 dark:text-orange-300',
    label: 'Sub',
  },
  insight: {
    bg: 'bg-indigo-100 dark:bg-indigo-900/30',
    text: 'text-indigo-700 dark:text-indigo-300',
    label: 'Insight',
  },
};

export function RecentActivity() {
  return (
    <section aria-label="Recent activity">
      <h2 className="mb-3 text-lg font-semibold text-gray-900 dark:text-white">Recent Activity</h2>
      <div className="rounded-xl border border-gray-200 bg-white dark:border-gray-700 dark:bg-gray-800">
        <ul
          className="max-h-80 divide-y divide-gray-100 overflow-y-auto dark:divide-gray-700"
          role="list"
        >
          {MOCK_ACTIVITY.map((event) => {
            const style = TYPE_STYLES[event.type];
            return (
              <li key={event.id} className="flex items-center gap-3 px-4 py-3">
                <span
                  className={`inline-flex shrink-0 items-center rounded-full px-2 py-0.5 text-xs font-medium ${style.bg} ${style.text}`}
                >
                  {style.label}
                </span>
                <p className="min-w-0 flex-1 truncate text-sm text-gray-700 dark:text-gray-300">
                  {event.message}
                </p>
                <time className="shrink-0 text-xs text-gray-400 dark:text-gray-500">
                  {event.timestamp}
                </time>
              </li>
            );
          })}
        </ul>
      </div>
    </section>
  );
}
