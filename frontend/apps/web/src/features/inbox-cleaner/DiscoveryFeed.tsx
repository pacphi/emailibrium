import { useEffect, useRef } from 'react';
import type { Discovery } from './hooks/useIngestionProgress';

interface DiscoveryFeedProps {
  discoveries: Discovery[];
  maxItems?: number;
}

const typeIcons: Record<Discovery['type'], string> = {
  subscription:
    'M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z',
  cluster:
    'M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10',
  pattern: 'M13 10V3L4 14h7v7l9-11h-7z',
};

const typeBadgeColors: Record<Discovery['type'], string> = {
  subscription: 'bg-indigo-100 text-indigo-700 dark:bg-indigo-900/40 dark:text-indigo-300',
  cluster: 'bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300',
  pattern: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300',
};

function formatTimestamp(ts: number): string {
  const date = new Date(ts);
  return date.toLocaleTimeString(undefined, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

export function DiscoveryFeed({ discoveries, maxItems = 50 }: DiscoveryFeedProps) {
  const bottomRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [discoveries.length]);

  const visibleDiscoveries = discoveries.slice(-maxItems);

  if (visibleDiscoveries.length === 0) {
    return (
      <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-6">
        <p className="text-sm text-gray-500 dark:text-gray-400 text-center">
          Waiting for discoveries...
        </p>
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 overflow-hidden"
    >
      <div className="px-4 py-3 border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50">
        <h3 className="text-sm font-semibold text-gray-700 dark:text-gray-300">Live Discoveries</h3>
      </div>
      <div className="max-h-64 overflow-y-auto p-2 space-y-1">
        {visibleDiscoveries.map((discovery) => (
          <div
            key={discovery.id}
            className="flex items-start gap-3 px-3 py-2 rounded-md hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors"
          >
            <svg
              className="w-4 h-4 mt-0.5 shrink-0 text-gray-400"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d={typeIcons[discovery.type]}
              />
            </svg>
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2">
                <span
                  className={`inline-flex px-1.5 py-0.5 text-[10px] font-medium rounded ${typeBadgeColors[discovery.type]}`}
                >
                  {discovery.type}
                </span>
                <span className="text-xs text-gray-400 dark:text-gray-500">
                  {formatTimestamp(discovery.timestamp)}
                </span>
              </div>
              <p className="text-sm text-gray-700 dark:text-gray-300 truncate">
                {discovery.message}
              </p>
            </div>
          </div>
        ))}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}
