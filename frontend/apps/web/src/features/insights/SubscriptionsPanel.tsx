import { useState } from 'react';
import type { SubscriptionInsight, UnsubscribeTarget } from '@emailibrium/types';
import { FrequencyBadge } from './components/FrequencyBadge';
import { UnsubscribePreviewDialog } from './components/UnsubscribePreviewDialog';
import { UndoToast } from './components/UndoToast';
import { useUnsubscribeFlow } from './hooks/useUnsubscribe';

interface SubscriptionsPanelProps {
  subscriptions: SubscriptionInsight[] | undefined;
  isLoading: boolean;
  onRefresh?: () => void;
}

interface SectionProps {
  title: string;
  count: number;
  tintClass: string;
  items: SubscriptionInsight[];
  bulkAction?: { label: string; onClick: () => void; disabled?: boolean };
  onUnsubscribeSingle?: (id: string) => void;
  onKeepSingle?: (id: string) => void;
  defaultExpanded?: boolean;
}

function ReadRateBar({ rate }: { rate: number }) {
  const pct = Math.round(rate * 100);
  const barColor = pct < 10 ? 'bg-red-500' : pct < 50 ? 'bg-yellow-500' : 'bg-green-500';

  return (
    <div className="flex items-center gap-2">
      <div className="h-2 w-20 overflow-hidden rounded-full bg-gray-200 dark:bg-gray-700">
        <div
          className={`h-full rounded-full ${barColor} transition-all`}
          style={{ width: `${pct}%` }}
        />
      </div>
      <span className="w-10 text-right text-xs text-gray-500 dark:text-gray-400">{pct}%</span>
    </div>
  );
}

function SubscriptionRow({
  item,
  onUnsubscribe,
  onKeep,
}: {
  item: SubscriptionInsight;
  onUnsubscribe?: (id: string) => void;
  onKeep?: (id: string) => void;
}) {
  const readRate = item.readRate ?? 0;
  const suggested = item.suggestedAction;

  return (
    <tr className="border-b border-gray-100 last:border-0 dark:border-gray-700/50">
      <td className="py-3 pr-4">
        <div>
          <p className="text-sm font-medium text-gray-900 dark:text-white">{item.senderAddress}</p>
          <p className="text-xs text-gray-500 dark:text-gray-400">{item.senderDomain}</p>
        </div>
      </td>
      <td className="py-3 pr-4">
        <FrequencyBadge frequency={item.frequency} />
      </td>
      <td className="py-3 pr-4 text-sm text-gray-600 dark:text-gray-300">
        {item.emailCount.toLocaleString()}
      </td>
      <td className="py-3 pr-4">
        <ReadRateBar rate={readRate} />
      </td>
      <td className="py-3 pr-4 text-xs text-gray-500 dark:text-gray-400">
        {new Date(item.lastSeen).toLocaleDateString()}
      </td>
      <td className="py-3">
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={() => onUnsubscribe?.(item.senderAddress)}
            className={`text-sm font-medium ${
              suggested === 'unsubscribe'
                ? 'text-red-600 hover:text-red-500 dark:text-red-400'
                : 'text-gray-400 hover:text-red-500 dark:text-gray-500 dark:hover:text-red-400'
            }`}
          >
            Unsubscribe
          </button>
          <span className="text-gray-300 dark:text-gray-600">|</span>
          <button
            type="button"
            onClick={() => onKeep?.(item.senderAddress)}
            className={`text-sm font-medium ${
              suggested === 'keep'
                ? 'text-green-600 hover:text-green-500 dark:text-green-400'
                : 'text-gray-400 hover:text-green-500 dark:text-gray-500 dark:hover:text-green-400'
            }`}
          >
            Keep
          </button>
        </div>
      </td>
    </tr>
  );
}

function SubscriptionSection({
  title,
  count,
  tintClass,
  items,
  bulkAction,
  onUnsubscribeSingle,
  onKeepSingle,
  defaultExpanded = false,
}: SectionProps) {
  const [expanded, setExpanded] = useState(defaultExpanded);

  return (
    <div className={`rounded-xl border ${tintClass} overflow-hidden`}>
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center justify-between px-5 py-4 text-left"
      >
        <div className="flex items-center gap-2">
          <svg
            className={`h-4 w-4 text-gray-500 transition-transform ${expanded ? 'rotate-90' : ''}`}
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            strokeWidth={2}
            aria-hidden="true"
          >
            <path strokeLinecap="round" strokeLinejoin="round" d="M9 5l7 7-7 7" />
          </svg>
          <h3 className="text-sm font-semibold text-gray-900 dark:text-white">{title}</h3>
          <span className="rounded-full bg-gray-200 px-2 py-0.5 text-xs font-medium text-gray-700 dark:bg-gray-600 dark:text-gray-200">
            {count}
          </span>
        </div>
        {bulkAction && (
          <span
            role="button"
            tabIndex={0}
            onClick={(e) => {
              e.stopPropagation();
              if (!bulkAction.disabled) bulkAction.onClick();
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.stopPropagation();
                if (!bulkAction.disabled) bulkAction.onClick();
              }
            }}
            className={`text-xs font-medium ${
              bulkAction.disabled
                ? 'cursor-not-allowed text-gray-400'
                : 'text-red-600 hover:text-red-500 dark:text-red-400'
            }`}
          >
            {bulkAction.label}
          </span>
        )}
      </button>
      {expanded && items.length > 0 && (
        <div className="overflow-x-auto px-5 pb-4">
          <table className="w-full min-w-[640px]">
            <thead>
              <tr className="text-left text-xs font-medium text-gray-500 dark:text-gray-400">
                <th className="pb-2 pr-4">Sender</th>
                <th className="pb-2 pr-4">Frequency</th>
                <th className="pb-2 pr-4">Emails</th>
                <th className="pb-2 pr-4">Read Rate</th>
                <th className="pb-2 pr-4">Last Seen</th>
                <th className="pb-2">Action</th>
              </tr>
            </thead>
            <tbody>
              {items.map((item) => (
                <SubscriptionRow
                  key={item.senderAddress}
                  item={item}
                  onUnsubscribe={onUnsubscribeSingle}
                  onKeep={onKeepSingle}
                />
              ))}
            </tbody>
          </table>
        </div>
      )}
      {expanded && items.length === 0 && (
        <p className="px-5 pb-4 text-sm text-gray-400">No subscriptions in this category.</p>
      )}
    </div>
  );
}

function PanelSkeleton() {
  return (
    <div className="animate-pulse space-y-4">
      <div className="grid grid-cols-3 gap-4">
        {Array.from({ length: 3 }).map((_, i) => (
          <div key={i} className="h-20 rounded-xl bg-gray-200 dark:bg-gray-700" />
        ))}
      </div>
      {Array.from({ length: 3 }).map((_, i) => (
        <div key={i} className="h-16 rounded-xl bg-gray-200 dark:bg-gray-700" />
      ))}
    </div>
  );
}

export function SubscriptionsPanel({
  subscriptions,
  isLoading,
  onRefresh: _onRefresh,
}: SubscriptionsPanelProps) {
  const {
    isPreviewOpen,
    isPreviewLoading,
    previewData,
    pendingTargets,
    openPreview,
    closePreview,
    confirmUnsubscribe,
    isUnsubscribing,
    undoState,
    isUndoing,
    handleUndo,
    dismissUndo,
  } = useUnsubscribeFlow();

  if (isLoading) return <PanelSkeleton />;

  const all = subscriptions ?? [];
  const totalEmails = all.reduce((sum, s) => sum + s.emailCount, 0);
  const estimatedHours = Math.round(totalEmails * 0.5) / 60; // ~30s per email

  // Partition by suggested action / open rate heuristic
  const neverOpened = all.filter((s) => s.suggestedAction === 'unsubscribe');
  const rarelyOpened = all.filter(
    (s) => s.suggestedAction === 'archive' || s.suggestedAction === 'digest',
  );
  const regularlyOpened = all.filter((s) => s.suggestedAction === 'keep');

  function toTarget(item: SubscriptionInsight): UnsubscribeTarget {
    return {
      sender: item.senderAddress,
      listUnsubscribeHeader: item.listUnsubscribe,
      listUnsubscribePost: item.listUnsubscribePost,
    };
  }

  function handleSingleUnsubscribe(senderAddress: string) {
    const item = all.find((s) => s.senderAddress === senderAddress);
    openPreview(item ? [toTarget(item)] : [{ sender: senderAddress }]);
  }

  function handleBulkUnsubscribe(items: SubscriptionInsight[]) {
    openPreview(items.map(toTarget));
  }

  function handleKeep(_senderAddress: string) {
    // TODO: persist "keep" decision to train the suggestion model
    // For now this is a no-op acknowledgement — the subscription stays.
  }

  return (
    <div className="space-y-6">
      {/* Summary cards */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <div className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm dark:border-gray-700 dark:bg-gray-800">
          <p className="text-sm text-gray-500 dark:text-gray-400">Total Subscriptions</p>
          <p className="text-2xl font-semibold text-gray-900 dark:text-white">{all.length}</p>
        </div>
        <div className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm dark:border-gray-700 dark:bg-gray-800">
          <p className="text-sm text-gray-500 dark:text-gray-400">Monthly Email Volume</p>
          <p className="text-2xl font-semibold text-gray-900 dark:text-white">
            {totalEmails.toLocaleString()}
          </p>
        </div>
        <div className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm dark:border-gray-700 dark:bg-gray-800">
          <p className="text-sm text-gray-500 dark:text-gray-400">Est. Reading Hours</p>
          <p className="text-2xl font-semibold text-gray-900 dark:text-white">
            {estimatedHours.toFixed(1)}h
          </p>
        </div>
      </div>

      {/* Trend */}
      <p className="text-sm text-gray-500 dark:text-gray-400">+3 new this month</p>

      {/* Sections */}
      <SubscriptionSection
        title="Never Opened"
        count={neverOpened.length}
        tintClass="border-red-200 bg-red-50/50 dark:border-red-900/40 dark:bg-red-900/10"
        items={neverOpened}
        onUnsubscribeSingle={handleSingleUnsubscribe}
        onKeepSingle={handleKeep}
        bulkAction={{
          label: isUnsubscribing ? 'Unsubscribing...' : 'Unsubscribe All',
          disabled: isUnsubscribing || neverOpened.length === 0,
          onClick: () => handleBulkUnsubscribe(neverOpened),
        }}
        defaultExpanded
      />

      <SubscriptionSection
        title="Rarely Opened (<10%)"
        count={rarelyOpened.length}
        tintClass="border-yellow-200 bg-yellow-50/50 dark:border-yellow-900/40 dark:bg-yellow-900/10"
        items={rarelyOpened}
        onUnsubscribeSingle={handleSingleUnsubscribe}
        onKeepSingle={handleKeep}
      />

      <SubscriptionSection
        title="Regularly Opened (>50%)"
        count={regularlyOpened.length}
        tintClass="border-green-200 bg-green-50/50 dark:border-green-900/40 dark:bg-green-900/10"
        items={regularlyOpened}
        onUnsubscribeSingle={handleSingleUnsubscribe}
        onKeepSingle={handleKeep}
      />

      {/* Unsubscribe preview dialog */}
      <UnsubscribePreviewDialog
        isOpen={isPreviewOpen}
        isLoading={isPreviewLoading}
        previews={previewData}
        pendingCount={pendingTargets.length}
        isUnsubscribing={isUnsubscribing}
        onConfirm={confirmUnsubscribe}
        onCancel={closePreview}
      />

      {/* Undo toast */}
      {undoState && (
        <UndoToast
          count={undoState.count}
          deadline={undoState.deadline}
          isUndoing={isUndoing}
          onUndo={handleUndo}
          onDismiss={dismissUndo}
        />
      )}
    </div>
  );
}
