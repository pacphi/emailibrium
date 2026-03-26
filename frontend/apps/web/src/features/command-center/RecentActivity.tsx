import type { EnrichedCategory } from '@emailibrium/api';

interface RecentActivityProps {
  categories?: EnrichedCategory[];
}

export function RecentActivity({ categories }: RecentActivityProps) {
  const hasCategories = categories && categories.length > 0;

  return (
    <section aria-label="Recent activity">
      <h2 className="mb-3 text-lg font-semibold text-gray-900 dark:text-white">
        Category Breakdown
      </h2>
      <div className="rounded-xl border border-gray-200 bg-white p-4 dark:border-gray-700 dark:bg-gray-800">
        {!hasCategories ? (
          <p className="mt-4 text-center text-sm text-gray-500 dark:text-gray-400">
            Categories will appear after emails are classified.
          </p>
        ) : (
          <div className="space-y-3">
            {categories.map((cat) => {
              const maxCount = Math.max(...categories.map((c) => c.emailCount), 1);
              const widthPct = Math.round((cat.emailCount / maxCount) * 100);
              return (
                <div key={cat.name}>
                  <div className="mb-1 flex items-center justify-between text-sm">
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-gray-900 dark:text-white">{cat.name}</span>
                      <span className="rounded-full bg-gray-100 px-2 py-0.5 text-xs text-gray-500 dark:bg-gray-700 dark:text-gray-400">
                        {cat.group}
                      </span>
                    </div>
                    <div className="flex items-center gap-3 text-xs text-gray-500 dark:text-gray-400">
                      <span>{cat.emailCount.toLocaleString()} emails</span>
                      {cat.unreadCount > 0 && (
                        <span className="font-medium text-indigo-600 dark:text-indigo-400">
                          {cat.unreadCount.toLocaleString()} unread
                        </span>
                      )}
                    </div>
                  </div>
                  <div className="h-2 overflow-hidden rounded-full bg-gray-100 dark:bg-gray-700">
                    <div
                      className="h-full rounded-full bg-indigo-500 transition-all dark:bg-indigo-400"
                      style={{ width: `${widthPct}%` }}
                    />
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </section>
  );
}
