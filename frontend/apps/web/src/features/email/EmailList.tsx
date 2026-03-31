import { useRef, useCallback, useEffect } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Loader2 } from 'lucide-react';
import type { Email } from '@emailibrium/types';
import { EmailListItem } from './EmailListItem';
import { useSettings, type EmailListDensity } from '../settings/hooks/useSettings';

const DENSITY_ROW_HEIGHT: Record<EmailListDensity, number> = {
  compact: 48,
  comfortable: 64,
  spacious: 84,
};

interface EmailListProps {
  emails: Email[];
  selectedEmailId: string | null;
  checkedEmailIds: Set<string>;
  isLoading: boolean;
  isError: boolean;
  hasNextPage?: boolean;
  isFetchingNextPage?: boolean;
  onFetchNextPage?: () => void;
  onSelectEmail: (emailId: string) => void;
  onCheckEmail: (emailId: string, checked: boolean) => void;
  onStarEmail: (emailId: string) => void;
  onArchiveEmail: (emailId: string) => void;
  onDeleteEmail: (emailId: string) => void;
  onMoveOpen?: (emailId: string) => void;
  onMarkUnread?: (emailId: string) => void;
  /** When set, scroll to this email in the list (used after "Show in inbox"). */
  scrollToEmailId?: string | null;
  onScrollToComplete?: () => void;
}

export function EmailList({
  emails,
  selectedEmailId,
  checkedEmailIds,
  isLoading,
  isError,
  onSelectEmail,
  onCheckEmail,
  onStarEmail,
  onArchiveEmail,
  onDeleteEmail,
  onMoveOpen,
  hasNextPage,
  isFetchingNextPage,
  onFetchNextPage,
  onMarkUnread,
  scrollToEmailId,
  onScrollToComplete,
}: EmailListProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const { emailListDensity, fontSize } = useSettings();
  const rowHeight = DENSITY_ROW_HEIGHT[emailListDensity];

  const virtualizer = useVirtualizer({
    count: emails.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => rowHeight,
    overscan: 10,
  });

  // Re-measure all rows when density changes.
  useEffect(() => {
    virtualizer.measure();
  }, [emailListDensity, virtualizer]);

  // Scroll to a specific email when requested (e.g. "Show in inbox" from search).
  useEffect(() => {
    if (!scrollToEmailId) return;
    const index = emails.findIndex((e) => e.id === scrollToEmailId);
    if (index >= 0) {
      virtualizer.scrollToIndex(index, { align: 'center', behavior: 'smooth' });
      onScrollToComplete?.();
    }
  }, [scrollToEmailId, emails, virtualizer, onScrollToComplete]);

  // Infinite scroll: load more when last few items are visible.
  const virtualItems = virtualizer.getVirtualItems();
  const lastItemIndex = virtualItems.at(-1)?.index;
  useEffect(() => {
    if (lastItemIndex == null) return;
    if (
      lastItemIndex >= emails.length - 5 &&
      hasNextPage &&
      !isFetchingNextPage &&
      onFetchNextPage
    ) {
      onFetchNextPage();
    }
  }, [lastItemIndex, emails.length, hasNextPage, isFetchingNextPage, onFetchNextPage]);

  const handleKeyNavigation = useCallback(
    (e: React.KeyboardEvent) => {
      if (!selectedEmailId || emails.length === 0) return;
      const currentIndex = emails.findIndex((em) => em.id === selectedEmailId);
      if (e.key === 'ArrowDown' && currentIndex < emails.length - 1) {
        e.preventDefault();
        const next = emails[currentIndex + 1];
        if (next) onSelectEmail(next.id);
      } else if (e.key === 'ArrowUp' && currentIndex > 0) {
        e.preventDefault();
        const prev = emails[currentIndex - 1];
        if (prev) onSelectEmail(prev.id);
      }
    },
    [selectedEmailId, emails, onSelectEmail],
  );

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

  if (emails.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center p-4">
        <p className="text-sm text-gray-500 dark:text-gray-400">No emails in this view.</p>
      </div>
    );
  }

  return (
    <div
      ref={parentRef}
      role="grid"
      aria-label="Email list"
      aria-rowcount={emails.length}
      onKeyDown={handleKeyNavigation}
      className="h-full overflow-auto"
    >
      <div
        style={{
          height: `${virtualizer.getTotalSize()}px`,
          width: '100%',
          position: 'relative',
        }}
      >
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const email = emails[virtualRow.index];
          if (!email) return null;
          return (
            <div
              key={email.id}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                height: `${virtualRow.size}px`,
                transform: `translateY(${virtualRow.start}px)`,
              }}
            >
              <EmailListItem
                email={email}
                isSelected={selectedEmailId === email.id}
                isChecked={checkedEmailIds.has(email.id)}
                density={emailListDensity}
                fontSize={fontSize}
                onSelect={onSelectEmail}
                onCheck={onCheckEmail}
                onStar={onStarEmail}
                onArchive={onArchiveEmail}
                onDelete={onDeleteEmail}
                onMoveOpen={onMoveOpen}
                onMarkUnread={onMarkUnread}
              />
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
