import type { SubscriptionInsight, RecurrencePattern, SuggestedAction } from '@emailibrium/types';

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

const actionLabels: Record<SuggestedAction, string> = {
  keep: 'Keep',
  unsubscribe: 'Unsubscribe',
  archive: 'Archive',
  digest: 'Digest',
};

const actionColors: Record<SuggestedAction, string> = {
  keep: 'text-green-600 dark:text-green-400',
  unsubscribe: 'text-red-600 dark:text-red-400',
  archive: 'text-amber-600 dark:text-amber-400',
  digest: 'text-blue-600 dark:text-blue-400',
};

function getInitials(address: string): string {
  const name = address.split('@')[0] ?? '';
  return name.slice(0, 2).toUpperCase();
}

function getDomainColor(domain: string): string {
  // Simple hash to pick a color
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

export function SubscriptionRow({ subscription, isSelected, onToggle }: SubscriptionRowProps) {
  const { senderAddress, senderDomain, frequency, emailCount, suggestedAction, hasUnsubscribe } =
    subscription;

  return (
    <label
      className={`flex items-center gap-3 px-4 py-3 rounded-lg border transition-colors cursor-pointer ${
        isSelected
          ? 'border-blue-300 bg-blue-50 dark:border-blue-700 dark:bg-blue-900/20'
          : 'border-gray-200 bg-white hover:border-gray-300 dark:border-gray-700 dark:bg-gray-800 dark:hover:border-gray-600'
      }`}
    >
      {/* Checkbox */}
      <input
        type="checkbox"
        checked={isSelected}
        onChange={() => onToggle(senderAddress)}
        className="h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500"
      />

      {/* Avatar */}
      <div
        className={`w-8 h-8 rounded-full flex items-center justify-center text-white text-xs font-bold shrink-0 ${getDomainColor(senderDomain)}`}
      >
        {getInitials(senderAddress)}
      </div>

      {/* Sender info */}
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
          {senderAddress}
        </p>
        <p className="text-xs text-gray-500 dark:text-gray-400">{senderDomain}</p>
      </div>

      {/* Frequency badge */}
      <span
        className={`hidden sm:inline-flex px-2 py-0.5 text-xs font-medium rounded-full ${frequencyColors[frequency]}`}
      >
        {frequencyLabels[frequency]}
      </span>

      {/* Email count */}
      <div className="text-right shrink-0 w-16">
        <p className="text-sm font-semibold text-gray-900 dark:text-gray-100">
          {emailCount.toLocaleString()}
        </p>
        <p className="text-[10px] text-gray-400">emails</p>
      </div>

      {/* Suggested action */}
      <span
        className={`hidden md:inline-flex text-xs font-medium w-20 justify-center ${actionColors[suggestedAction]}`}
      >
        {actionLabels[suggestedAction]}
      </span>

      {/* Unsubscribe indicator */}
      {hasUnsubscribe && (
        <span
          className="hidden lg:inline-flex text-[10px] text-gray-400 dark:text-gray-500"
          title="Has unsubscribe link"
        >
          <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.878 9.878L3 3m6.878 6.878L21 21"
            />
          </svg>
        </span>
      )}
    </label>
  );
}
