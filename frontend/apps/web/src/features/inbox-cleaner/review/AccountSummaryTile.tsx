import type { CleanupProvider } from '@emailibrium/types';

export interface AccountSummaryTileProps {
  accountId: string;
  accountLabel: string;
  provider: CleanupProvider;
  counts: {
    archive?: number;
    addLabel?: number;
    move?: number;
    delete?: number;
    unsubscribe?: number;
    markRead?: number;
    star?: number;
  };
  risk: { low: number; medium: number; high: number };
}

const providerLabels: Record<CleanupProvider, string> = {
  gmail: 'Gmail',
  outlook: 'Outlook',
  imap: 'IMAP',
  pop3: 'POP3',
};

const actionLabels: Record<keyof AccountSummaryTileProps['counts'], string> = {
  archive: 'Archive',
  addLabel: 'Label',
  move: 'Move',
  delete: 'Delete',
  unsubscribe: 'Unsubscribe',
  markRead: 'Mark read',
  star: 'Star',
};

export function AccountSummaryTile({
  accountId,
  accountLabel,
  provider,
  counts,
  risk,
}: AccountSummaryTileProps) {
  const total = risk.low + risk.medium + risk.high;
  const lowPct = total > 0 ? (risk.low / total) * 100 : 0;
  const medPct = total > 0 ? (risk.medium / total) * 100 : 0;
  const highPct = total > 0 ? (risk.high / total) * 100 : 0;

  const actionEntries = (Object.keys(counts) as Array<keyof typeof counts>)
    .filter((k) => (counts[k] ?? 0) > 0)
    .map((k) => ({ key: k, label: actionLabels[k], count: counts[k]! }));

  return (
    <article
      className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4 space-y-3"
      aria-labelledby={`account-${accountId}-label`}
    >
      <header className="flex items-center justify-between gap-2">
        <div className="min-w-0">
          <h3
            id={`account-${accountId}-label`}
            className="text-sm font-semibold text-gray-900 dark:text-gray-100 truncate"
          >
            {accountLabel}
          </h3>
          <p className="text-xs text-gray-500 dark:text-gray-400">{providerLabels[provider]}</p>
        </div>
        <span className="text-xs font-semibold text-gray-700 dark:text-gray-300 shrink-0">
          {total.toLocaleString()} ops
        </span>
      </header>

      {actionEntries.length > 0 && (
        <ul role="list" className="flex flex-wrap gap-2">
          {actionEntries.map(({ key, label, count }) => (
            <li
              key={key}
              role="listitem"
              className="inline-flex items-center gap-1 rounded-md bg-gray-100 dark:bg-gray-700 px-2 py-0.5 text-xs text-gray-700 dark:text-gray-200"
            >
              <span className="font-medium">{label}:</span>
              <span>{count.toLocaleString()}</span>
            </li>
          ))}
        </ul>
      )}

      <div>
        <div
          className="flex w-full overflow-hidden rounded-full h-2 bg-gray-200 dark:bg-gray-700"
          role="img"
          aria-label={`Risk distribution: ${risk.low} low, ${risk.medium} medium, ${risk.high} high`}
        >
          {risk.low > 0 && <div className="bg-green-500 h-full" style={{ width: `${lowPct}%` }} />}
          {risk.medium > 0 && (
            <div className="bg-amber-500 h-full" style={{ width: `${medPct}%` }} />
          )}
          {risk.high > 0 && <div className="bg-red-500 h-full" style={{ width: `${highPct}%` }} />}
        </div>
        <div className="mt-1 flex justify-between text-[10px] text-gray-500 dark:text-gray-400">
          <span>Low {risk.low}</span>
          <span>Medium {risk.medium}</span>
          <span>High {risk.high}</span>
        </div>
      </div>
    </article>
  );
}
