import type { SubscriptionInsight, RecurrencePattern } from '@emailibrium/types';

interface SubscriptionRowProps {
  subscription: SubscriptionInsight;
  isSelected: boolean;
  onToggle: (senderAddress: string) => void;
}

const frequencyLabels: Record<RecurrencePattern, string> = {
  daily: 'Daily',
  weekly: 'Weekly',
  biweekly: 'Biweekly',
  monthly: 'Monthly',
  quarterly: 'Quarterly',
  irregular: 'Irregular',
};

const frequencyColors: Record<RecurrencePattern, string> = {
  daily: 'bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300',
  weekly: 'bg-orange-100 text-orange-700 dark:bg-orange-900/40 dark:text-orange-300',
  biweekly: 'bg-yellow-100 text-yellow-700 dark:bg-yellow-900/40 dark:text-yellow-300',
  monthly: 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300',
  quarterly: 'bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-300',
  irregular: 'bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300',
};

function getInitials(address: string): string {
  const name = address.split('@')[0] ?? '';
  return name.slice(0, 2).toUpperCase();
}

function getDomainColor(domain: string): string {
  let hash = 0;
  for (let i = 0; i < domain.length; i++) {
    hash = domain.charCodeAt(i) + ((hash << 5) - hash);
  }
  const colors = [
    'bg-indigo-500',
    'bg-pink-500',
    'bg-teal-500',
    'bg-orange-500',
    'bg-cyan-500',
    'bg-violet-500',
    'bg-emerald-500',
    'bg-rose-500',
  ];
  return colors[Math.abs(hash) % colors.length]!;
}

// Grid template: avatar | sender | frequency | count | action | indicator
// Using CSS grid so every row shares identical column widths — flex with optional
// elements would shift column positions when hasUnsubscribe differs between rows.
const GRID_COLS = '[grid-template-columns:2rem_1fr_5.5rem_4rem_7.5rem_1.25rem]';

export function SubscriptionRow({ subscription, isSelected, onToggle }: SubscriptionRowProps) {
  const { senderAddress, senderDomain, frequency, emailCount, hasUnsubscribe } = subscription;

  const currentAction = isSelected ? 'unsubscribe' : 'keep';

  const handleActionChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    e.stopPropagation();
    const shouldBeSelected = e.target.value === 'unsubscribe';
    if (shouldBeSelected !== isSelected) {
      onToggle(senderAddress);
    }
  };

  return (
    <div
      className={`grid items-center gap-x-3 px-4 py-3 rounded-lg border transition-colors ${GRID_COLS} ${
        isSelected
          ? 'border-blue-300 bg-blue-50 dark:border-blue-700 dark:bg-blue-900/20'
          : 'border-gray-200 bg-white dark:border-gray-700 dark:bg-gray-800'
      }`}
    >
      {/* Col 1 — Avatar */}
      <div
        className={`w-8 h-8 rounded-full flex items-center justify-center text-white text-xs font-bold ${getDomainColor(senderDomain)}`}
      >
        {getInitials(senderAddress)}
      </div>

      {/* Col 2 — Sender info */}
      <div className="min-w-0">
        <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
          {senderAddress}
        </p>
        <p className="text-xs text-gray-500 dark:text-gray-400 truncate">{senderDomain}</p>
      </div>

      {/* Col 3 — Frequency badge */}
      <span
        className={`inline-flex justify-center px-2 py-0.5 text-xs font-medium rounded-full ${frequencyColors[frequency]}`}
      >
        {frequencyLabels[frequency]}
      </span>

      {/* Col 4 — Email count */}
      <div className="text-right">
        <p className="text-sm font-semibold text-gray-900 dark:text-gray-100">
          {emailCount.toLocaleString()}
        </p>
        <p className="text-[10px] text-gray-400">emails</p>
      </div>

      {/* Col 5 — Action dropdown */}
      <select
        value={currentAction}
        onChange={handleActionChange}
        onClick={(e) => e.stopPropagation()}
        className="w-full text-xs rounded-md border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-gray-700 dark:text-gray-200 px-2 py-1 focus:outline-none focus:ring-1 focus:ring-blue-500"
      >
        <option value="keep">Keep</option>
        <option value="unsubscribe">Unsubscribe</option>
      </select>

      {/* Col 6 — Unsubscribe indicator (always occupies the column; invisible when absent) */}
      <div className="flex justify-center">
        {hasUnsubscribe ? (
          <span className="text-gray-400 dark:text-gray-500" title="Has unsubscribe link">
            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.878 9.878L3 3m6.878 6.878L21 21"
              />
            </svg>
          </span>
        ) : null}
      </div>
    </div>
  );
}
