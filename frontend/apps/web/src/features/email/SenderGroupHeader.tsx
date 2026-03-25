import { useState } from 'react';
import { ChevronRight, ChevronDown, Archive, Trash2, FolderInput, MailOpen, Mail } from 'lucide-react';

interface SenderGroupHeaderProps {
  displayName: string;
  fromAddr: string;
  emailCount: number;
  unreadCount: number;
  isCollapsed: boolean;
  onToggle: () => void;
  provider: string;
  onBulkArchive?: () => void;
  onBulkDelete?: () => void;
  onBulkMove?: () => void;
  onBulkMarkRead?: () => void;
  onBulkMarkUnread?: () => void;
}

const providerBadge: Record<string, { label: string; className: string }> = {
  gmail: {
    label: 'G',
    className: 'bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300',
  },
  outlook: {
    label: 'M',
    className: 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300',
  },
  imap: {
    label: 'I',
    className: 'bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-300',
  },
};

function getInitials(name?: string, email?: string): string {
  const source = name || email || '?';
  const chars = source
    .split(/[\s@.]+/)
    .filter(Boolean)
    .map((w) => {
      const match = w.match(/[A-Za-z0-9]/);
      return match ? match[0].toUpperCase() : '';
    })
    .filter(Boolean);
  return chars.slice(0, 2).join('') || '?';
}

export function SenderGroupHeader({
  displayName,
  fromAddr,
  emailCount,
  unreadCount,
  isCollapsed,
  onToggle,
  provider,
  onBulkArchive,
  onBulkDelete,
  onBulkMove,
  onBulkMarkRead,
  onBulkMarkUnread,
}: SenderGroupHeaderProps) {
  const [showActions, setShowActions] = useState(false);
  const badge = providerBadge[provider] ?? providerBadge['imap']!;
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
      className={`
        flex h-12 cursor-pointer items-center gap-3 border-b border-gray-200 bg-gray-50 px-3
        transition-colors hover:bg-gray-100
        dark:border-gray-700 dark:bg-gray-750 dark:hover:bg-gray-700
      `}
    >
      {/* Collapse chevron */}
      <Chevron className="h-4 w-4 shrink-0 text-gray-400 dark:text-gray-500" />

      {/* Avatar */}
      <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-indigo-100 text-xs font-semibold text-indigo-700 dark:bg-indigo-900 dark:text-indigo-200">
        {getInitials(displayName, fromAddr)}
      </div>

      {/* Provider badge */}
      <span
        className={`inline-flex h-4 w-4 shrink-0 items-center justify-center rounded text-[10px] font-bold ${badge.className}`}
        title={provider}
      >
        {badge.label}
      </span>

      {/* Sender info */}
      <div className="flex min-w-0 items-center gap-2">
        <span className="truncate text-sm font-medium text-gray-900 dark:text-white">
          {displayName || fromAddr}
        </span>
        {displayName && fromAddr && (
          <span className="truncate text-xs text-gray-500 dark:text-gray-400">
            {fromAddr}
          </span>
        )}
      </div>

      {/* Spacer */}
      <div className="flex-1" />

      {/* Hover actions or count pill */}
      {showActions ? (
        <div className="flex items-center gap-1">
          {unreadCount > 0 && onBulkMarkRead && (
            <button
              type="button"
              onClick={(e) => { e.stopPropagation(); onBulkMarkRead(); }}
              className="rounded p-1 text-gray-400 hover:bg-gray-200 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
              aria-label={`Mark all read from ${displayName}`}
              title="Mark all read"
            >
              <MailOpen className="h-3.5 w-3.5" />
            </button>
          )}
          {unreadCount === 0 && onBulkMarkUnread && (
            <button
              type="button"
              onClick={(e) => { e.stopPropagation(); onBulkMarkUnread(); }}
              className="rounded p-1 text-gray-400 hover:bg-gray-200 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
              aria-label={`Mark all unread from ${displayName}`}
              title="Mark all unread"
            >
              <Mail className="h-3.5 w-3.5" />
            </button>
          )}
          {onBulkArchive && (
            <button
              type="button"
              onClick={(e) => { e.stopPropagation(); onBulkArchive(); }}
              className="rounded p-1 text-gray-400 hover:bg-gray-200 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
              aria-label={`Archive all from ${displayName}`}
              title="Archive all"
            >
              <Archive className="h-3.5 w-3.5" />
            </button>
          )}
          {onBulkDelete && (
            <button
              type="button"
              onClick={(e) => { e.stopPropagation(); onBulkDelete(); }}
              className="rounded p-1 text-gray-400 hover:bg-red-100 hover:text-red-600 dark:hover:bg-red-900/30 dark:hover:text-red-400"
              aria-label={`Delete all from ${displayName}`}
              title="Delete all"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          )}
          {onBulkMove && (
            <button
              type="button"
              onClick={(e) => { e.stopPropagation(); onBulkMove(); }}
              className="rounded p-1 text-gray-400 hover:bg-gray-200 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
              aria-label={`Move all from ${displayName}`}
              title="Move all"
            >
              <FolderInput className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      ) : (
        <span className="inline-flex h-5 min-w-[20px] shrink-0 items-center justify-center rounded-full bg-indigo-100 px-1.5 text-xs font-medium text-indigo-700 dark:bg-indigo-900/50 dark:text-indigo-300">
          {emailCount}
        </span>
      )}
    </div>
  );
}
