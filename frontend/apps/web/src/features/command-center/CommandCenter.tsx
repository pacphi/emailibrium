import { useState, useCallback, useEffect } from 'react';
import { StatsCards } from './StatsCards';
import { QuickActions } from './QuickActions';
import { RecentActivity } from './RecentActivity';
import { ClusterVisualization } from './ClusterVisualization';
import { CommandPalette } from './CommandPalette';
import { SearchResults } from './SearchResults';
import { SyncStatusIndicator } from './SyncStatusIndicator';
import { useStats } from './hooks/useStats';
import { useCommandPalette } from './hooks/useCommandPalette';
import { useSyncStore } from '@/shared/stores/syncStore';

type View = 'dashboard' | 'search';

export function CommandCenter() {
  const [view, setView] = useState<View>('dashboard');
  const { stats, isLoading, isError, error, refetch } = useStats();
  const { open: openPalette } = useCommandPalette();

  // Global sync state — persists across navigation.
  const syncing = useSyncStore((s) => s.syncing);
  const syncStatus = useSyncStore((s) => s.status);
  const syncError = useSyncStore((s) => s.error);
  const hasAccounts = useSyncStore((s) => s.hasAccounts);
  const startSync = useSyncStore((s) => s.startSync);
  const clearError = useSyncStore((s) => s.clearError);
  const refreshAccounts = useSyncStore((s) => s.refreshAccounts);

  // Check for accounts on mount.
  useEffect(() => {
    refreshAccounts();
  }, [refreshAccounts]);

  // Refetch stats when sync completes.
  useEffect(() => {
    if (!syncing && syncStatus === 'Sync complete!') {
      refetch();
    }
  }, [syncing, syncStatus, refetch]);

  const handleQuickAction = useCallback(
    (actionId: string) => {
      if (actionId === 'sync-now') {
        startSync();
      }
    },
    [startSync],
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

          {/* Sync status banner */}
          {(syncStatus || syncError) && (
            <div
              className={`flex items-center gap-3 rounded-xl border px-4 py-3 text-sm ${
                syncError
                  ? 'border-red-200 bg-red-50 text-red-700 dark:border-red-800 dark:bg-red-900/20 dark:text-red-400'
                  : 'border-indigo-200 bg-indigo-50 text-indigo-700 dark:border-indigo-800 dark:bg-indigo-900/20 dark:text-indigo-400'
              }`}
              role="status"
            >
              {syncing && (
                <svg
                  className="h-4 w-4 animate-spin"
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
              )}
              <span>{syncError || syncStatus}</span>
              {syncError && (
                <button
                  type="button"
                  onClick={clearError}
                  className="ml-auto font-medium underline"
                >
                  Dismiss
                </button>
              )}
            </div>
          )}

          {/* Stats cards */}
          <StatsCards stats={stats} isLoading={isLoading} />

          {/* Quick actions */}
          <QuickActions onAction={handleQuickAction} syncing={syncing} hasAccounts={hasAccounts} />

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
