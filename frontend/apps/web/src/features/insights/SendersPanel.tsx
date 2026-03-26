import { useState, useMemo } from 'react';
import type { SubscriptionInsight } from '@emailibrium/types';

interface SendersPanelProps {
  senders: SubscriptionInsight[] | undefined;
  isLoading: boolean;
}

type SortField = 'sender' | 'emailCount' | 'frequency' | 'category' | 'lastSeen';
type SortDirection = 'asc' | 'desc';

const FREQUENCY_ORDER: Record<string, number> = {
  daily: 1,
  weekly: 2,
  biweekly: 3,
  monthly: 4,
  quarterly: 5,
  irregular: 6,
};

function SortIcon({ active, direction }: { active: boolean; direction: SortDirection }) {
  if (!active) {
    return (
      <svg
        className="ml-1 inline h-3 w-3 text-gray-400"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        strokeWidth={2}
        aria-hidden="true"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          d="M7 16V4m0 0L3 8m4-4l4 4m6 0v12m0 0l4-4m-4 4l-4-4"
        />
      </svg>
    );
  }
  return (
    <svg
      className="ml-1 inline h-3 w-3 text-indigo-600 dark:text-indigo-400"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d={direction === 'asc' ? 'M5 15l7-7 7 7' : 'M19 9l-7 7-7-7'}
      />
    </svg>
  );
}

function computeAvgInterval(frequency: string): string {
  switch (frequency) {
    case 'daily':
      return '1 day';
    case 'weekly':
      return '7 days';
    case 'biweekly':
      return '14 days';
    case 'monthly':
      return '30 days';
    case 'quarterly':
      return '90 days';
    default:
      return 'Varies';
  }
}

function PanelSkeleton() {
  return (
    <div className="animate-pulse space-y-4">
      <div className="h-10 w-64 rounded-lg bg-gray-200 dark:bg-gray-700" />
      <div className="space-y-2">
        {Array.from({ length: 8 }).map((_, i) => (
          <div key={i} className="h-12 rounded-lg bg-gray-200 dark:bg-gray-700" />
        ))}
      </div>
    </div>
  );
}

export function SendersPanel({ senders, isLoading }: SendersPanelProps) {
  const [search, setSearch] = useState('');
  const [sortField, setSortField] = useState<SortField>('emailCount');
  const [sortDir, setSortDir] = useState<SortDirection>('desc');

  const handleSort = (field: SortField) => {
    if (sortField === field) {
      setSortDir(sortDir === 'asc' ? 'desc' : 'asc');
    } else {
      setSortField(field);
      setSortDir('desc');
    }
  };

  const filtered = useMemo(() => {
    const items = senders ?? [];
    const q = search.toLowerCase().trim();
    const searched = q
      ? items.filter(
          (s) =>
            s.senderAddress.toLowerCase().includes(q) ||
            s.senderDomain.toLowerCase().includes(q) ||
            s.category.toLowerCase().includes(q),
        )
      : items;

    return [...searched].sort((a, b) => {
      const dir = sortDir === 'asc' ? 1 : -1;
      switch (sortField) {
        case 'sender':
          return dir * a.senderAddress.localeCompare(b.senderAddress);
        case 'emailCount':
          return dir * (a.emailCount - b.emailCount);
        case 'frequency':
          return (
            dir * ((FREQUENCY_ORDER[a.frequency] ?? 99) - (FREQUENCY_ORDER[b.frequency] ?? 99))
          );
        case 'category':
          return dir * a.category.localeCompare(b.category);
        case 'lastSeen':
          return dir * (new Date(a.lastSeen).getTime() - new Date(b.lastSeen).getTime());
        default:
          return 0;
      }
    });
  }, [senders, search, sortField, sortDir]);

  if (isLoading) return <PanelSkeleton />;

  // Hide categories when embeddings aren't active (all unknown/uncategorized).
  const hasRealCategories =
    senders?.some(
      (s) =>
        s.category !== 'unknown' &&
        s.category.toLowerCase() !== 'uncategorized',
    ) ?? false;

  const headers: Array<{ field: SortField; label: string }> = [
    { field: 'sender', label: 'Sender' },
    { field: 'emailCount', label: 'Emails' },
    { field: 'frequency', label: 'Avg Interval' },
    ...(hasRealCategories ? [{ field: 'category' as SortField, label: 'Category' }] : []),
    { field: 'lastSeen', label: 'Last Email' },
  ];

  return (
    <div className="space-y-4">
      {/* Search */}
      <div className="relative max-w-sm">
        <svg
          className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400"
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
        <input
          type="text"
          placeholder="Search senders..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="w-full rounded-lg border border-gray-200 bg-white py-2 pl-10 pr-4 text-sm text-gray-900 placeholder-gray-400 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500 dark:border-gray-700 dark:bg-gray-800 dark:text-white dark:placeholder-gray-500"
        />
      </div>

      {/* Table with sticky header and scrollable body */}
      <div className="max-h-[70vh] overflow-auto rounded-xl border border-gray-200 bg-white shadow-sm dark:border-gray-700 dark:bg-gray-800">
        <table className="w-full min-w-[640px]">
          <thead className="sticky top-0 z-10 bg-white dark:bg-gray-800">
            <tr className="border-b border-gray-200 dark:border-gray-700">
              {headers.map(({ field, label }) => (
                <th
                  key={field}
                  className="cursor-pointer px-5 py-3 text-left text-xs font-medium text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
                  onClick={() => handleSort(field)}
                >
                  {label}
                  <SortIcon active={sortField === field} direction={sortDir} />
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {filtered.length === 0 && (
              <tr>
                <td colSpan={headers.length} className="px-5 py-12 text-center text-sm text-gray-400">
                  {search ? 'No senders match your search.' : 'No sender data available.'}
                </td>
              </tr>
            )}
            {filtered.map((sender) => (
              <tr
                key={sender.senderAddress}
                className="cursor-pointer border-b border-gray-100 transition-colors hover:bg-gray-50 last:border-0 dark:border-gray-700/50 dark:hover:bg-gray-700/30"
              >
                <td className="px-5 py-3">
                  <p className="text-sm font-medium text-gray-900 dark:text-white">
                    {sender.senderAddress}
                  </p>
                  <p className="text-xs text-gray-500 dark:text-gray-400">{sender.senderDomain}</p>
                </td>
                <td className="px-5 py-3 text-sm text-gray-600 dark:text-gray-300">
                  {sender.emailCount.toLocaleString()}
                </td>
                <td className="px-5 py-3 text-sm text-gray-600 dark:text-gray-300">
                  {computeAvgInterval(sender.frequency)}
                </td>
                {hasRealCategories && (
                  <td className="px-5 py-3">
                    <span className="inline-flex rounded-full bg-gray-100 px-2 py-0.5 text-xs font-medium capitalize text-gray-700 dark:bg-gray-700 dark:text-gray-300">
                      {sender.category}
                    </span>
                  </td>
                )}
                <td className="px-5 py-3 text-sm text-gray-500 dark:text-gray-400">
                  {new Date(sender.lastSeen).toLocaleDateString()}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
