import { useState, useCallback } from 'react';
import { StatsCards } from './StatsCards';
import { QuickActions } from './QuickActions';
import { RecentActivity } from './RecentActivity';
import { ClusterVisualization } from './ClusterVisualization';
import { CommandPalette } from './CommandPalette';
import { SearchResults } from './SearchResults';
import { SyncStatusIndicator } from './SyncStatusIndicator';
import { useStats } from './hooks/useStats';
import { useCommandPalette } from './hooks/useCommandPalette';
import { getAccounts, startIngestion } from '@emailibrium/api';

type View = 'dashboard' | 'search';

export function CommandCenter() {
  const [view, setView] = useState<View>('dashboard');
  const { stats, isLoading, isError, error, refetch } = useStats();
  const { open: openPalette } = useCommandPalette();
  const [syncing, setSyncing] = useState(false);

  const handleQuickAction = useCallback(
    async (actionId: string) => {
      if (actionId === 'sync-now') {
        setSyncing(true);
        try {
          const accounts = await getAccounts();
          const active = accounts.filter((a) => a.isActive);
          if (active.length === 0) {
            window.location.href = '/onboarding';
            return;
          }
          await Promise.all(active.map((a) => startIngestion(a.id)));
          refetch();
        } catch {
          // ingestion may not be fully wired yet — fail silently
        } finally {
          setSyncing(false);
        }
      }
    },
    [refetch],
  );

  return (
    <div className="min-h-screen bg-gray-50 dark:bg-gray-900">
      {/* Command palette overlay -- always mounted, controlled by Zustand state */}
      <CommandPalette />

      {/* Header */}
      <header className="border-b border-gray-200 bg-white px-6 py-4 dark:border-gray-700 dark:bg-gray-800">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Command Center</h1>
            <p className="mt-0.5 text-sm text-gray-500 dark:text-gray-400">
              Your email intelligence dashboard
            </p>
          </div>
          <div className="flex items-center gap-3">
            {/* Sync status */}
            <SyncStatusIndicator />

            {/* View toggle */}
            <div
              className="flex rounded-lg border border-gray-200 dark:border-gray-600"
              role="group"
              aria-label="View toggle"
            >
              <button
                type="button"
                onClick={() => setView('dashboard')}
                className={`rounded-l-lg px-3 py-1.5 text-sm font-medium transition-colors focus:outline-none focus:ring-2 focus:ring-indigo-500 ${
                  view === 'dashboard'
                    ? 'bg-indigo-600 text-white'
                    : 'bg-white text-gray-600 hover:bg-gray-50 dark:bg-gray-800 dark:text-gray-300'
                }`}
                aria-pressed={view === 'dashboard'}
              >
                Dashboard
              </button>
              <button
                type="button"
                onClick={() => setView('search')}
                className={`rounded-r-lg px-3 py-1.5 text-sm font-medium transition-colors focus:outline-none focus:ring-2 focus:ring-indigo-500 ${
                  view === 'search'
                    ? 'bg-indigo-600 text-white'
                    : 'bg-white text-gray-600 hover:bg-gray-50 dark:bg-gray-800 dark:text-gray-300'
                }`}
                aria-pressed={view === 'search'}
              >
                Search
              </button>
            </div>

            {/* Command palette trigger */}
            <button
              type="button"
              onClick={openPalette}
              className="flex items-center gap-2 rounded-lg border border-gray-200 bg-white px-3 py-1.5 text-sm text-gray-500 shadow-sm transition-colors hover:bg-gray-50 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:ring-offset-2 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-400 dark:hover:bg-gray-600"
              aria-label="Open command palette"
            >
              <svg
                className="h-4 w-4"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
                strokeWidth={2}
                aria-hidden="true"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
                />
              </svg>
              <span className="hidden sm:inline">Search...</span>
              <kbd className="hidden rounded bg-gray-100 px-1.5 py-0.5 text-xs sm:inline-block dark:bg-gray-600">
                {navigator.platform?.includes('Mac') ? '\u2318' : 'Ctrl+'}K
              </kbd>
            </button>
          </div>
        </div>
      </header>

      {/* Main content */}
      {view === 'search' ? (
        <SearchResults />
      ) : (
        <div className="space-y-6 p-6">
          {/* Error banner */}
          {isError && (
            <div
              className="flex items-center justify-between rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700 dark:border-red-800 dark:bg-red-900/20 dark:text-red-400"
              role="alert"
            >
              <span>
                Failed to load statistics:{' '}
                {error instanceof Error ? error.message : 'Unknown error'}
              </span>
              <button
                type="button"
                onClick={() => refetch()}
                className="font-medium underline hover:text-red-800 dark:hover:text-red-300"
              >
                Retry
              </button>
            </div>
          )}

          {/* Stats cards */}
          <StatsCards stats={stats} isLoading={isLoading} />

          {/* Quick actions */}
          <QuickActions onAction={handleQuickAction} syncing={syncing} />

          {/* Two-column layout for activity and clusters */}
          <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
            <RecentActivity />
            <ClusterVisualization isLoading={isLoading} />
          </div>
        </div>
      )}
    </div>
  );
}
