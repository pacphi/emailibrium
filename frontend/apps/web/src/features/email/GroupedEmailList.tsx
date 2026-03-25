import { useRef, useMemo, useEffect } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Loader2 } from 'lucide-react';
import type { DomainGroup, VirtualItem } from './utils/groupBySender';
import { flattenGroups } from './utils/groupBySender';
import { DomainGroupHeader } from './DomainGroupHeader';
import { SenderGroupHeader } from './SenderGroupHeader';
import { EmailListItem } from './EmailListItem';

interface GroupedEmailListProps {
  domains: DomainGroup[];
  expandedDomains: Set<string>;
  expandedSenders: Set<string>;
  onToggleDomain: (domain: string) => void;
  onToggleSender: (groupKey: string) => void;
  selectedEmailId: string | null;
  checkedEmailIds: Set<string>;
  onSelectEmail: (emailId: string) => void;
  onCheckEmail: (emailId: string, checked: boolean) => void;
  onStarEmail: (emailId: string) => void;
  onArchiveEmail: (emailId: string) => void;
  onDeleteEmail: (emailId: string) => void;
  onMoveOpen: (emailId: string) => void;
  onMarkUnread: (emailId: string) => void;
  onBulkArchive: (emailIds: string[]) => void;
  onBulkDelete: (emailIds: string[]) => void;
  onBulkMoveOpen: (emailIds: string[], subject: string) => void;
  onBulkMarkRead: (emailIds: string[]) => void;
  onBulkMarkUnread: (emailIds: string[]) => void;
  isLoading: boolean;
  isError: boolean;
  hasNextPage?: boolean;
  isFetchingNextPage?: boolean;
  onFetchNextPage?: () => void;
}

const ITEM_HEIGHT: Record<VirtualItem['type'], number> = {
  'domain-header': 44,
  'sender-header': 48,
  'email': 64,
};

export function GroupedEmailList({
  domains,
  expandedDomains,
  expandedSenders,
  onToggleDomain,
  onToggleSender,
  selectedEmailId,
  checkedEmailIds,
  onSelectEmail,
  onCheckEmail,
  onStarEmail,
  onArchiveEmail,
  onDeleteEmail,
  onMoveOpen,
  onMarkUnread,
  onBulkArchive,
  onBulkDelete,
  onBulkMoveOpen,
  onBulkMarkRead,
  onBulkMarkUnread,
  isLoading,
  isError,
  hasNextPage,
  isFetchingNextPage,
  onFetchNextPage,
}: GroupedEmailListProps) {
  const scrollRef = useRef<HTMLDivElement>(null);

  const flatItems: VirtualItem[] = useMemo(
    () => flattenGroups(domains, expandedDomains, expandedSenders),
    [domains, expandedDomains, expandedSenders],
  );

  const virtualizer = useVirtualizer({
    count: flatItems.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) => ITEM_HEIGHT[flatItems[index]?.type ?? 'email'],
    overscan: 10,
  });

  // Infinite scroll: load more when near the bottom of the virtual list
  useEffect(() => {
    const lastItem = virtualizer.getVirtualItems().at(-1);
    if (!lastItem) return;
    if (
      lastItem.index >= flatItems.length - 5 &&
      hasNextPage &&
      !isFetchingNextPage &&
      onFetchNextPage
    ) {
      onFetchNextPage();
    }
  }, [
    virtualizer.getVirtualItems(),
    flatItems.length,
    hasNextPage,
    isFetchingNextPage,
    onFetchNextPage,
  ]);

  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-6 w-6 animate-spin text-indigo-500" />
        <span className="ml-2 text-sm text-gray-500">Loading emails...</span>
      </div>
    );
  }

  if (isError) {
    return (
      <div className="flex h-full items-center justify-center p-4">
        <p className="text-sm text-red-500">Failed to load emails. Please try again.</p>
      </div>
    );
  }

  if (domains.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center p-4">
        <p className="text-sm text-gray-500 dark:text-gray-400">No emails to display</p>
      </div>
    );
  }

  return (
    <div
      ref={scrollRef}
      role="grid"
      aria-label="Grouped email list"
      aria-rowcount={flatItems.length}
      className="flex-1 overflow-y-auto"
    >
      <div style={{ height: `${virtualizer.getTotalSize()}px`, width: '100%', position: 'relative' }}>
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const item = flatItems[virtualRow.index];
          if (!item) return null;

          let content: React.ReactNode;

          if (item.type === 'domain-header') {
            const domainEmails = item.domain.senderGroups.flatMap((g) => g.emails);
            const domainEmailIds = domainEmails.map((e) => e.id);
            const domainUnread = domainEmails.filter((e) => !e.isRead).length;
            content = (
              <DomainGroupHeader
                domain={item.domain.domain}
                senderCount={item.domain.senderGroups.length}
                totalEmails={item.domain.totalEmails}
                unreadCount={domainUnread}
                isCollapsed={!expandedDomains.has(item.domain.domain)}
                onToggle={() => onToggleDomain(item.domain.domain)}
                onBulkArchive={() => onBulkArchive(domainEmailIds)}
                onBulkDelete={() => onBulkDelete(domainEmailIds)}
                onBulkMove={() =>
                  onBulkMoveOpen(
                    domainEmailIds,
                    `${item.domain.totalEmails} emails from ${item.domain.domain}`,
                  )
                }
                onBulkMarkRead={() => onBulkMarkRead(domainEmailIds)}
                onBulkMarkUnread={() => onBulkMarkUnread(domainEmailIds)}
              />
            );
          } else if (item.type === 'sender-header') {
            const senderEmailIds = item.group.emails.map((e) => e.id);
            const senderUnread = item.group.emails.filter((e) => !e.isRead).length;
            content = (
              <SenderGroupHeader
                displayName={item.group.displayName}
                fromAddr={item.group.fromAddr}
                emailCount={item.group.emails.length}
                unreadCount={senderUnread}
                isCollapsed={!expandedSenders.has(item.group.key)}
                onToggle={() => onToggleSender(item.group.key)}
                provider={item.group.provider}
                onBulkArchive={() => onBulkArchive(senderEmailIds)}
                onBulkDelete={() => onBulkDelete(senderEmailIds)}
                onBulkMove={() =>
                  onBulkMoveOpen(
                    senderEmailIds,
                    `${item.group.emails.length} emails from ${item.group.displayName}`,
                  )
                }
                onBulkMarkRead={() => onBulkMarkRead(senderEmailIds)}
                onBulkMarkUnread={() => onBulkMarkUnread(senderEmailIds)}
              />
            );
          } else {
            content = (
              <EmailListItem
                email={item.email}
                isSelected={selectedEmailId === item.email.id}
                isChecked={checkedEmailIds.has(item.email.id)}
                onSelect={onSelectEmail}
                onCheck={onCheckEmail}
                onStar={onStarEmail}
                onArchive={onArchiveEmail}
                onDelete={onDeleteEmail}
                onMoveOpen={onMoveOpen}
                onMarkUnread={onMarkUnread}
              />
            );
          }

          return (
            <div
              key={
                item.type === 'domain-header'
                  ? `domain-${item.domain.domain}`
                  : item.type === 'sender-header'
                  ? `sender-${item.group.key}`
                  : `email-${item.email.id}`
              }
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                height: `${virtualRow.size}px`,
                transform: `translateY(${virtualRow.start}px)`,
              }}
            >
              {content}
            </div>
          );
        })}
      </div>
      {isFetchingNextPage && (
        <div className="flex items-center justify-center py-3">
          <Loader2 className="h-4 w-4 animate-spin text-indigo-500" />
          <span className="ml-2 text-xs text-gray-500">Loading more...</span>
        </div>
      )}
    </div>
  );
}
