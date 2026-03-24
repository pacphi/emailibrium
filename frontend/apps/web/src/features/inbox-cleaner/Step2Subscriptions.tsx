import type { SubscriptionInsight } from '@emailibrium/types';
import { SubscriptionRow } from './SubscriptionRow';

interface Step2SubscriptionsProps {
  categorized: {
    neverOpened: SubscriptionInsight[];
    rarelyOpened: SubscriptionInsight[];
    regularlyOpened: SubscriptionInsight[];
  };
  selectedSubscriptions: Set<string>;
  onToggle: (senderAddress: string) => void;
  onSelectAll: (subs: SubscriptionInsight[]) => void;
  onDeselectAll: (subs: SubscriptionInsight[]) => void;
  isLoading: boolean;
  error: Error | null;
}

interface SectionProps {
  title: string;
  description: string;
  subscriptions: SubscriptionInsight[];
  selectedSubscriptions: Set<string>;
  onToggle: (senderAddress: string) => void;
  onSelectAll: (subs: SubscriptionInsight[]) => void;
  onDeselectAll: (subs: SubscriptionInsight[]) => void;
  badgeColor: string;
}

function Section({
  title,
  description,
  subscriptions,
  selectedSubscriptions,
  onToggle,
  onSelectAll,
  onDeselectAll,
  badgeColor,
}: SectionProps) {
  if (subscriptions.length === 0) return null;

  const allSelected = subscriptions.every((s) => selectedSubscriptions.has(s.senderAddress));
  const selectedCount = subscriptions.filter((s) =>
    selectedSubscriptions.has(s.senderAddress),
  ).length;

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">{title}</h3>
          <span
            className={`inline-flex px-2 py-0.5 text-xs font-medium rounded-full ${badgeColor}`}
          >
            {subscriptions.length}
          </span>
          {selectedCount > 0 && (
            <span className="text-xs text-gray-500 dark:text-gray-400">
              ({selectedCount} selected)
            </span>
          )}
        </div>
        <button
          onClick={() => (allSelected ? onDeselectAll(subscriptions) : onSelectAll(subscriptions))}
          className="text-xs font-medium text-blue-600 hover:text-blue-700 dark:text-blue-400 dark:hover:text-blue-300"
        >
          {allSelected ? 'Deselect All' : 'Select All'}
        </button>
      </div>
      <p className="text-xs text-gray-500 dark:text-gray-400">{description}</p>
      <div className="space-y-2">
        {subscriptions.map((sub) => (
          <SubscriptionRow
            key={sub.senderAddress}
            subscription={sub}
            isSelected={selectedSubscriptions.has(sub.senderAddress)}
            onToggle={onToggle}
          />
        ))}
      </div>
    </div>
  );
}

export function Step2Subscriptions({
  categorized,
  selectedSubscriptions,
  onToggle,
  onSelectAll,
  onDeselectAll,
  isLoading,
  error,
}: Step2SubscriptionsProps) {
  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <div className="flex flex-col items-center gap-3">
          <div className="w-8 h-8 border-4 border-blue-500 border-t-transparent rounded-full animate-spin" />
          <p className="text-sm text-gray-500 dark:text-gray-400">Loading subscriptions...</p>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="rounded-lg border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/20 p-6 text-center">
        <p className="text-sm text-red-600 dark:text-red-400">
          Failed to load subscriptions: {error.message}
        </p>
      </div>
    );
  }

  const totalSubs =
    categorized.neverOpened.length +
    categorized.rarelyOpened.length +
    categorized.regularlyOpened.length;

  if (totalSubs === 0) {
    return (
      <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-8 text-center">
        <p className="text-sm text-gray-500 dark:text-gray-400">
          No subscriptions detected yet. Complete the ingestion step first.
        </p>
      </div>
    );
  }

  const totalSelectedEmails = [...selectedSubscriptions].reduce((sum, addr) => {
    const allSubs = [
      ...categorized.neverOpened,
      ...categorized.rarelyOpened,
      ...categorized.regularlyOpened,
    ];
    const sub = allSubs.find((s) => s.senderAddress === addr);
    return sum + (sub?.emailCount ?? 0);
  }, 0);

  return (
    <div className="space-y-6">
      {/* Summary banner */}
      {selectedSubscriptions.size > 0 && (
        <div className="rounded-lg bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 px-4 py-3">
          <p className="text-sm text-blue-700 dark:text-blue-300">
            <span className="font-semibold">{selectedSubscriptions.size}</span> subscription
            {selectedSubscriptions.size !== 1 ? 's' : ''} selected covering{' '}
            <span className="font-semibold">{totalSelectedEmails.toLocaleString()}</span> emails
          </p>
        </div>
      )}

      {/* Never Opened section */}
      <Section
        title="Never Opened"
        description="You have never opened emails from these senders. Safe to unsubscribe."
        subscriptions={categorized.neverOpened}
        selectedSubscriptions={selectedSubscriptions}
        onToggle={onToggle}
        onSelectAll={onSelectAll}
        onDeselectAll={onDeselectAll}
        badgeColor="bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300"
      />

      {/* Rarely Opened section */}
      <Section
        title="Rarely Opened"
        description="Opened less than 10% of the time. Consider unsubscribing or archiving."
        subscriptions={categorized.rarelyOpened}
        selectedSubscriptions={selectedSubscriptions}
        onToggle={onToggle}
        onSelectAll={onSelectAll}
        onDeselectAll={onDeselectAll}
        badgeColor="bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300"
      />

      {/* Regularly Opened section */}
      <Section
        title="Regularly Opened"
        description="Opened more than 50% of the time. You likely want to keep these."
        subscriptions={categorized.regularlyOpened}
        selectedSubscriptions={selectedSubscriptions}
        onToggle={onToggle}
        onSelectAll={onSelectAll}
        onDeselectAll={onDeselectAll}
        badgeColor="bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300"
      />
    </div>
  );
}
