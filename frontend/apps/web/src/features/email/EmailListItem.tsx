import { forwardRef, useState } from 'react';
import { Star, Archive, Trash2, FolderInput, Paperclip, MailOpen } from 'lucide-react';
import { formatDistanceToNow } from 'date-fns';
import type { Email } from '@emailibrium/types';
import type { EmailListDensity } from '../settings/hooks/useSettings';

interface EmailListItemProps {
  email: Email;
  isSelected: boolean;
  isChecked: boolean;
  density?: EmailListDensity;
  fontSize?: number;
  onSelect: (emailId: string) => void;
  onCheck: (emailId: string, checked: boolean) => void;
  onStar: (emailId: string) => void;
  onArchive: (emailId: string) => void;
  onDelete: (emailId: string) => void;
  onMoveOpen?: (emailId: string) => void;
  onMarkUnread?: (emailId: string) => void;
}

const DENSITY_PADDING: Record<EmailListDensity, string> = {
  compact: 'py-1 px-3',
  comfortable: 'py-2.5 px-3',
  spacious: 'py-4 px-3',
};

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
  // Split into words, extract first alphanumeric char from each.
  const chars = source
    .split(/[\s@.]+/)
    .filter(Boolean)
    .map((w) => {
      const match = w.match(/[A-Za-z0-9]/);
      return match ? match[0].toUpperCase() : '';
    })
    .filter(Boolean);
  // Use up to 2 characters; if only one word, use single char.
  return chars.slice(0, 2).join('') || '?';
}

function getPreview(email: Email): string {
  const text = email.bodyText ?? '';
  return text.slice(0, 120).replace(/\s+/g, ' ').trim();
}

export const EmailListItem = forwardRef<HTMLDivElement, EmailListItemProps>(
  function EmailListItem(
    { email, isSelected, isChecked, density = 'comfortable', fontSize = 14, onSelect, onCheck, onStar, onArchive, onDelete, onMoveOpen, onMarkUnread },
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
        style={{ fontSize: `${fontSize}px` }}
        className={`
          group flex cursor-pointer items-center gap-3 border-b border-gray-100 ${DENSITY_PADDING[density]} transition-colors
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

        {/* Avatar */}
        <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-indigo-100 text-xs font-semibold text-indigo-700 dark:bg-indigo-900 dark:text-indigo-200">
          {getInitials(email.fromName, email.fromAddr)}
        </div>

        {/* Content */}
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span
              className={`inline-flex h-4 w-4 shrink-0 items-center justify-center rounded text-[10px] font-bold ${badge.className}`}
              title={email.provider}
            >
              {badge.label}
            </span>
            <span
              className={`truncate ${
                !email.isRead
                  ? 'font-semibold text-gray-900 dark:text-white'
                  : 'font-medium text-gray-700 dark:text-gray-300'
              }`}
            >
              {email.fromName || email.fromAddr}
            </span>
          </div>
          <div className="flex items-center gap-1">
            <span
              className={`truncate ${
                !email.isRead
                  ? 'font-medium text-gray-900 dark:text-white'
                  : 'text-gray-600 dark:text-gray-400'
              }`}
            >
              {email.subject}
            </span>
            {preview && (
              <span className="truncate text-gray-400 dark:text-gray-500">
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
              {email.isRead && onMarkUnread && (
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    onMarkUnread(email.id);
                  }}
                  className="rounded p-1 text-gray-400 hover:bg-gray-200 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
                  aria-label="Mark as unread"
                  title="Mark as unread"
                >
                  <MailOpen className="h-4 w-4" />
                </button>
              )}
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
