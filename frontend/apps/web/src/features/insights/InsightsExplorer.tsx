import { useState, lazy, Suspense } from 'react';
import { useInboxReport, useSubscriptions, useRecurringSenders } from './hooks/useInsights';

const OverviewPanel = lazy(() =>
  import('./OverviewPanel').then((m) => ({ default: m.OverviewPanel })),
);
const SubscriptionsPanel = lazy(() =>
  import('./SubscriptionsPanel').then((m) => ({ default: m.SubscriptionsPanel })),
);
const SendersPanel = lazy(() =>
  import('./SendersPanel').then((m) => ({ default: m.SendersPanel })),
);
const TopicsPanel = lazy(() => import('./TopicsPanel').then((m) => ({ default: m.TopicsPanel })));
const TrendsPanel = lazy(() => import('./TrendsPanel').then((m) => ({ default: m.TrendsPanel })));

type TabKey = 'overview' | 'subscriptions' | 'senders' | 'topics' | 'trends';

interface Tab {
  key: TabKey;
  label: string;
}

const TABS: Tab[] = [
  { key: 'overview', label: 'Overview' },
  { key: 'subscriptions', label: 'Subscriptions' },
  { key: 'senders', label: 'Senders' },
  { key: 'topics', label: 'Topics' },
  { key: 'trends', label: 'Trends' },
];

function TabSkeleton() {
  return (
    <div className="animate-pulse space-y-4">
      <div className="h-64 rounded-xl bg-gray-200 dark:bg-gray-700" />
      <div className="h-48 rounded-xl bg-gray-200 dark:bg-gray-700" />
    </div>
  );
}

export function InsightsExplorer() {
  const [activeTab, setActiveTab] = useState<TabKey>('overview');
  const { data: report, isLoading: reportLoading } = useInboxReport();
  const { data: subscriptions, isLoading: subsLoading } = useSubscriptions();
  const { data: senders, isLoading: sendersLoading } = useRecurringSenders();

  return (
    <div className="mx-auto max-w-7xl space-y-6 px-4 py-6 sm:px-6 lg:px-8">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Insights Explorer</h1>
        <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
          Analyze your email patterns, subscriptions, and trends.
        </p>
      </div>

      {/* Tabs */}
      <nav
        className="flex gap-1 rounded-lg border border-gray-200 bg-gray-50 p-1 dark:border-gray-700 dark:bg-gray-800/50"
        aria-label="Insights tabs"
      >
        {TABS.map((tab) => (
          <button
            key={tab.key}
            onClick={() => setActiveTab(tab.key)}
            className={`rounded-md px-4 py-2 text-sm font-medium transition-colors ${
              activeTab === tab.key
                ? 'bg-white text-gray-900 shadow-sm dark:bg-gray-700 dark:text-white'
                : 'text-gray-600 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white'
            }`}
            aria-selected={activeTab === tab.key}
            role="tab"
          >
            {tab.label}
          </button>
        ))}
      </nav>

      {/* Panel content */}
      <Suspense fallback={<TabSkeleton />}>
        {activeTab === 'overview' && <OverviewPanel report={report} isLoading={reportLoading} />}
        {activeTab === 'subscriptions' && (
          <SubscriptionsPanel subscriptions={subscriptions} isLoading={subsLoading} />
        )}
        {activeTab === 'senders' && <SendersPanel senders={senders} isLoading={sendersLoading} />}
        {activeTab === 'topics' && <TopicsPanel senders={senders} isLoading={sendersLoading} />}
        {activeTab === 'trends' && <TrendsPanel report={report} isLoading={reportLoading} />}
      </Suspense>
    </div>
  );
}
