import { forwardRef, useState } from 'react';
import { Star, Archive, Trash2, FolderInput, Paperclip } from 'lucide-react';
import { formatDistanceToNow } from 'date-fns';
import type { Email } from '@emailibrium/types';

interface EmailListItemProps {
  email: Email;
  isSelected: boolean;
  isChecked: boolean;
  onSelect: (emailId: string) => void;
  onCheck: (emailId: string, checked: boolean) => void;
  onStar: (emailId: string) => void;
  onArchive: (emailId: string) => void;
  onDelete: (emailId: string) => void;
  onMoveOpen?: (emailId: string) => void;
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
  return source
    .split(/[\s@.]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((s) => s[0]?.toUpperCase() ?? '')
    .join('');
}

function getPreview(email: Email): string {
  const text = email.bodyText ?? '';
  return text.slice(0, 120).replace(/\s+/g, ' ').trim();
}

export const EmailListItem = forwardRef<HTMLDivElement, EmailListItemProps>(
  function EmailListItem(
    { email, isSelected, isChecked, onSelect, onCheck, onStar, onArchive, onDelete, onMoveOpen },
    ref,
  ) {
    const [showActions, setShowActions] = useState(false);
    const badge = providerBadge[email.provider] ?? providerBadge['imap']!;
    const preview = getPreview(email);
    const dateLabel = formatDistanceToNow(new Date(email.receivedAt), {
      addSuffix: true,
    });

    return (
      <div
        ref={ref}
        role="row"
        tabIndex={0}
        aria-selected={isSelected}
        onClick={() => onSelect(email.id)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            onSelect(email.id);
          }
        }}
        onMouseEnter={() => setShowActions(true)}
        onMouseLeave={() => setShowActions(false)}
        className={`
          group flex cursor-pointer items-center gap-3 border-b border-gray-100 px-3 py-2.5 transition-colors
          dark:border-gray-700/50
          ${
            isSelected
              ? 'bg-indigo-50 dark:bg-indigo-900/30'
              : 'hover:bg-gray-50 dark:hover:bg-gray-800/50'
          }
          ${!email.isRead ? 'bg-white dark:bg-gray-800' : 'bg-gray-50/50 dark:bg-gray-850'}
        `}
      >
        {/* Checkbox */}
        <input
          type="checkbox"
          checked={isChecked}
          onChange={(e) => {
            e.stopPropagation();
            onCheck(email.id, e.target.checked);
          }}
          onClick={(e) => e.stopPropagation()}
          className="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
          aria-label={`Select ${email.subject}`}
        />

        {/* Star */}
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            onStar(email.id);
          }}
          className={`shrink-0 ${
            email.isStarred
              ? 'text-yellow-400'
              : 'text-gray-300 hover:text-yellow-400 dark:text-gray-600'
          }`}
          aria-label={email.isStarred ? 'Unstar email' : 'Star email'}
          title={email.isStarred ? 'Unstar' : 'Star'}
        >
          <Star className="h-4 w-4" fill={email.isStarred ? 'currentColor' : 'none'} />
        </button>

        {/* Unread indicator */}
        {!email.isRead && (
          <span className="h-2 w-2 shrink-0 rounded-full bg-indigo-500" aria-label="Unread" />
        )}

        {/* Avatar */}
        <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-indigo-100 text-xs font-semibold text-indigo-700 dark:bg-indigo-900 dark:text-indigo-200">
          {getInitials(email.fromName, email.fromAddr)}
        </div>

        {/* Content */}
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span
              className={`truncate text-sm ${
                !email.isRead
                  ? 'font-semibold text-gray-900 dark:text-white'
                  : 'font-medium text-gray-700 dark:text-gray-300'
              }`}
            >
              {email.fromName || email.fromAddr}
            </span>
            <span
              className={`inline-flex h-4 w-4 items-center justify-center rounded text-[10px] font-bold ${badge.className}`}
              title={email.provider}
            >
              {badge.label}
            </span>
          </div>
          <div className="flex items-center gap-1">
            <span
              className={`truncate text-sm ${
                !email.isRead
                  ? 'font-medium text-gray-900 dark:text-white'
                  : 'text-gray-600 dark:text-gray-400'
              }`}
            >
              {email.subject}
            </span>
            {preview && (
              <span className="truncate text-sm text-gray-400 dark:text-gray-500">
                {' '}
                - {preview}
              </span>
            )}
          </div>
        </div>

        {/* Right side: date, attachment, hover actions */}
        <div className="flex shrink-0 items-center gap-2">
          {email.hasAttachments && (
            <Paperclip
              className="h-3.5 w-3.5 text-gray-400 dark:text-gray-500"
              aria-label="Has attachments"
            />
          )}

          {showActions ? (
            <div className="flex items-center gap-1">
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  onArchive(email.id);
                }}
                className="rounded p-1 text-gray-400 hover:bg-gray-200 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
                aria-label="Archive"
                title="Archive"
              >
                <Archive className="h-4 w-4" />
              </button>
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  onDelete(email.id);
                }}
                className="rounded p-1 text-gray-400 hover:bg-red-100 hover:text-red-600 dark:hover:bg-red-900/30 dark:hover:text-red-400"
                aria-label="Delete"
                title="Delete"
              >
                <Trash2 className="h-4 w-4" />
              </button>
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  onMoveOpen?.(email.id);
                }}
                className="rounded p-1 text-gray-400 hover:bg-gray-200 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
                aria-label="Move to folder"
                title="Move to folder"
              >
                <FolderInput className="h-4 w-4" />
              </button>
            </div>
          ) : (
            <span className="whitespace-nowrap text-xs text-gray-400 dark:text-gray-500">
              {dateLabel}
            </span>
          )}
        </div>
      </div>
    );
  },
);
