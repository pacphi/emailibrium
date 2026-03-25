import { useRef, useCallback, useEffect } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Loader2 } from 'lucide-react';
import type { Email } from '@emailibrium/types';
import { EmailListItem } from './EmailListItem';

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
}: EmailListProps) {
  const parentRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: emails.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 64,
    overscan: 10,
  });

  // Infinite scroll: load more when last few items are visible.
  useEffect(() => {
    const lastItem = virtualizer.getVirtualItems().at(-1);
    if (!lastItem) return;
    if (
      lastItem.index >= emails.length - 5 &&
      hasNextPage &&
      !isFetchingNextPage &&
      onFetchNextPage
    ) {
      onFetchNextPage();
    }
  }, [
    virtualizer.getVirtualItems(),
    emails.length,
    hasNextPage,
    isFetchingNextPage,
    onFetchNextPage,
  ]);

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
