import { useState } from 'react';
import {
  ChevronRight,
  ChevronDown,
  Archive,
  Trash2,
  FolderInput,
  MailOpen,
  Mail,
} from 'lucide-react';

interface DomainGroupHeaderProps {
  domain: string;
  senderCount: number;
  totalEmails: number;
  unreadCount: number;
  isCollapsed: boolean;
  onToggle: () => void;
  onBulkArchive?: () => void;
  onBulkDelete?: () => void;
  onBulkMove?: () => void;
  onBulkMarkRead?: () => void;
  onBulkMarkUnread?: () => void;
}

export function DomainGroupHeader({
  domain,
  senderCount,
  totalEmails,
  unreadCount,
  isCollapsed,
  onToggle,
  onBulkArchive,
  onBulkDelete,
  onBulkMove,
  onBulkMarkRead,
  onBulkMarkUnread,
}: DomainGroupHeaderProps) {
  const [showActions, setShowActions] = useState(false);
  const Chevron = isCollapsed ? ChevronRight : ChevronDown;

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onToggle}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onToggle();
        }
      }}
      onMouseEnter={() => setShowActions(true)}
      onMouseLeave={() => setShowActions(false)}
      aria-expanded={!isCollapsed}
      className="flex h-11 cursor-pointer items-center gap-2 border-b border-gray-300 bg-gray-100 px-3 transition-colors hover:bg-gray-200 dark:border-gray-600 dark:bg-gray-800 dark:hover:bg-gray-750"
    >
      <Chevron className="h-4 w-4 shrink-0 text-gray-500 dark:text-gray-400" />

      <span className="min-w-0 truncate text-sm font-semibold text-gray-800 dark:text-gray-100">
        {domain}
      </span>

      <div className="flex-1" />

      {showActions ? (
        <div className="flex items-center gap-1">
          {unreadCount > 0 && onBulkMarkRead && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onBulkMarkRead();
              }}
              className="rounded p-1 text-gray-400 hover:bg-gray-300 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
              aria-label={`Mark all read from ${domain}`}
              title="Mark all read"
            >
              <MailOpen className="h-3.5 w-3.5" />
            </button>
          )}
          {unreadCount === 0 && onBulkMarkUnread && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onBulkMarkUnread();
              }}
              className="rounded p-1 text-gray-400 hover:bg-gray-300 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
              aria-label={`Mark all unread from ${domain}`}
              title="Mark all unread"
            >
              <Mail className="h-3.5 w-3.5" />
            </button>
          )}
          {onBulkArchive && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onBulkArchive();
              }}
              className="rounded p-1 text-gray-400 hover:bg-gray-300 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
              aria-label={`Archive all from ${domain}`}
              title="Archive all"
            >
              <Archive className="h-3.5 w-3.5" />
            </button>
          )}
          {onBulkDelete && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onBulkDelete();
              }}
              className="rounded p-1 text-gray-400 hover:bg-red-100 hover:text-red-600 dark:hover:bg-red-900/30 dark:hover:text-red-400"
              aria-label={`Delete all from ${domain}`}
              title="Delete all"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          )}
          {onBulkMove && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onBulkMove();
              }}
              className="rounded p-1 text-gray-400 hover:bg-gray-300 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
              aria-label={`Move all from ${domain}`}
              title="Move all"
            >
              <FolderInput className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      ) : (
        <>
          <span className="shrink-0 text-xs text-gray-500 dark:text-gray-400">
            {senderCount} {senderCount === 1 ? 'sender' : 'senders'}
          </span>
          <span className="inline-flex h-5 min-w-[20px] shrink-0 items-center justify-center rounded-full bg-gray-300 px-1.5 text-xs font-medium text-gray-700 dark:bg-gray-600 dark:text-gray-200">
            {totalEmails}
          </span>
        </>
      )}
    </div>
  );
}
