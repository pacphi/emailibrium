import { type ReactNode, useState, useEffect } from 'react';
import type { AppStats } from './hooks/useStats';
import { getAccounts, getEmailCounts } from '@emailibrium/api';
import type { EmailCounts } from '@emailibrium/api';

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
  const [accountCount, setAccountCount] = useState<number | null>(null);
  const [accountsByProvider, setAccountsByProvider] = useState<Record<string, number>>({});
  const [emailCounts, setEmailCounts] = useState<EmailCounts | null>(null);

  useEffect(() => {
    getAccounts()
      .then((accounts) => {
        setAccountCount(accounts.length);
        const byProvider: Record<string, number> = {};
        for (const a of accounts) {
          byProvider[a.provider] = (byProvider[a.provider] || 0) + 1;
        }
        setAccountsByProvider(byProvider);
      })
      .catch(() => setAccountCount(0));

    getEmailCounts()
      .then((counts) => setEmailCounts(counts))
      .catch(() => {});
  }, []);

  // Refresh email counts periodically (every 10s) to reflect sync progress.
  useEffect(() => {
    const interval = setInterval(() => {
      getEmailCounts()
        .then((counts) => setEmailCounts(counts))
        .catch(() => {});
    }, 10_000);
    return () => clearInterval(interval);
  }, []);

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
      value: accountCount?.toString() ?? '0',
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

  const totalAccounts = accountCount ?? 0;

  return (
    <div className="space-y-4">
      <div
        className="grid grid-cols-2 gap-4 lg:grid-cols-4 xl:grid-cols-8"
        aria-label="Email statistics"
      >
        {cards.map((card) => (
          <StatCard key={card.label} {...card} />
        ))}
      </div>

      {totalAccounts > 0 && Object.keys(accountsByProvider).length > 0 && (
        <div className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-700 dark:bg-gray-800">
          <p className="text-sm font-medium text-gray-500 dark:text-gray-400 mb-3">
            Accounts by Provider
          </p>
          <div className="flex items-center gap-6">
            {/* Simple donut chart using SVG */}
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
            {/* Legend */}
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
                  <span className="text-sm font-medium text-gray-900 dark:text-white">{count}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
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
