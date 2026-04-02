import { useState } from 'react';
import {
  Archive,
  Star,
  Tag,
  FolderInput,
  Trash2,
  ChevronDown,
  ShieldAlert,
  RotateCcw,
  AlertTriangle,
} from 'lucide-react';

export type EmailViewContext = 'inbox' | 'spam' | 'trash' | 'sent';

interface EmailActionsProps {
  emailId: string | null;
  selectedCount: number;
  viewContext?: EmailViewContext;
  onArchive: () => void;
  onStar: () => void;
  onDelete: () => void;
  onReclassify: (category: string) => void;
  onMove: (groupId: string) => void;
  onSpam?: () => void;
  onRestore?: () => void;
  onPermanentDelete?: () => void;
}

const categories = [
  'Personal',
  'Work',
  'Finance',
  'Shopping',
  'Social',
  'Newsletter',
  'Marketing',
  'Notification',
  'Alerts',
  'Promotions',
  'Travel',
];

const groups = [
  { id: 'inbox', label: 'Inbox' },
  { id: 'archive', label: 'Archive' },
  { id: 'important', label: 'Important' },
  { id: 'later', label: 'Read Later' },
];

function DropdownButton({
  label,
  icon: Icon,
  items,
  onSelect,
}: {
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  items: { id: string; label: string }[];
  onSelect: (id: string) => void;
}) {
  const [isOpen, setIsOpen] = useState(false);

  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => setIsOpen((prev) => !prev)}
        className="flex items-center gap-1 rounded-md px-2.5 py-1.5 text-sm text-gray-600 transition-colors hover:bg-gray-100 dark:text-gray-300 dark:hover:bg-gray-700"
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
      >
        <Icon className="h-4 w-4" aria-hidden="true" />
        <span className="hidden sm:inline">{label}</span>
        <ChevronDown className="h-3 w-3" aria-hidden="true" />
      </button>
      {isOpen && (
        <>
          {/* Backdrop */}
          <div className="fixed inset-0 z-10" onClick={() => setIsOpen(false)} aria-hidden="true" />
          <ul
            role="listbox"
            className="absolute left-0 top-full z-20 mt-1 w-44 rounded-md border border-gray-200 bg-white py-1 shadow-lg dark:border-gray-600 dark:bg-gray-800"
          >
            {items.map((item) => (
              <li key={item.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={false}
                  onClick={() => {
                    onSelect(item.id);
                    setIsOpen(false);
                  }}
                  className="block w-full px-3 py-1.5 text-left text-sm text-gray-700 hover:bg-gray-100 dark:text-gray-300 dark:hover:bg-gray-700"
                >
                  {item.label}
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

export function EmailActions({
  emailId,
  selectedCount,
  viewContext = 'inbox',
  onArchive,
  onStar,
  onDelete,
  onReclassify,
  onMove,
  onSpam,
  onRestore,
  onPermanentDelete,
}: EmailActionsProps) {
  const [showPermanentConfirm, setShowPermanentConfirm] = useState(false);
  const hasTarget = emailId !== null || selectedCount > 0;
  const targetLabel = selectedCount > 1 ? `${selectedCount} emails` : 'email';

  if (!hasTarget) return null;

  const isTrashView = viewContext === 'trash';
  const isSpamView = viewContext === 'spam';
  const isSpecialView = isTrashView || isSpamView;

  return (
    <div
      className="flex items-center gap-1 border-b border-gray-200 bg-white px-3 py-2 dark:border-gray-700 dark:bg-gray-800"
      role="toolbar"
      aria-label={`Actions for ${targetLabel}`}
    >
      {/* Restore button -- visible only in Trash and Spam views */}
      {isSpecialView && onRestore && (
        <button
          type="button"
          onClick={onRestore}
          className="flex items-center gap-1 rounded-md px-2.5 py-1.5 text-sm text-green-600 transition-colors hover:bg-green-50 dark:text-green-400 dark:hover:bg-green-900/20"
          aria-label="Restore"
        >
          <RotateCcw className="h-4 w-4" aria-hidden="true" />
          <span className="hidden sm:inline">Restore</span>
        </button>
      )}

      {/* Standard actions -- hidden in special views */}
      {!isSpecialView && (
        <>
          <button
            type="button"
            onClick={onArchive}
            className="flex items-center gap-1 rounded-md px-2.5 py-1.5 text-sm text-gray-600 transition-colors hover:bg-gray-100 dark:text-gray-300 dark:hover:bg-gray-700"
            aria-label="Archive"
          >
            <Archive className="h-4 w-4" aria-hidden="true" />
            <span className="hidden sm:inline">Archive</span>
          </button>

          <button
            type="button"
            onClick={onStar}
            className="flex items-center gap-1 rounded-md px-2.5 py-1.5 text-sm text-gray-600 transition-colors hover:bg-gray-100 dark:text-gray-300 dark:hover:bg-gray-700"
            aria-label="Star"
          >
            <Star className="h-4 w-4" aria-hidden="true" />
            <span className="hidden sm:inline">Star</span>
          </button>

          <DropdownButton
            label="Reclassify"
            icon={Tag}
            items={categories.map((c) => ({ id: c.toLowerCase(), label: c }))}
            onSelect={onReclassify}
          />

          <DropdownButton label="Move" icon={FolderInput} items={groups} onSelect={onMove} />

          {/* Spam button -- visible in inbox view */}
          {onSpam && (
            <button
              type="button"
              onClick={onSpam}
              className="flex items-center gap-1 rounded-md px-2.5 py-1.5 text-sm text-amber-600 transition-colors hover:bg-amber-50 dark:text-amber-400 dark:hover:bg-amber-900/20"
              aria-label="Report spam"
            >
              <ShieldAlert className="h-4 w-4" aria-hidden="true" />
              <span className="hidden sm:inline">Spam</span>
            </button>
          )}
        </>
      )}

      <div className="mx-1 h-5 w-px bg-gray-200 dark:bg-gray-600" aria-hidden="true" />

      {/* Permanent Delete button -- visible only in Trash view */}
      {isTrashView && onPermanentDelete && (
        <div className="relative">
          <button
            type="button"
            onClick={() => setShowPermanentConfirm(true)}
            className="flex items-center gap-1 rounded-md px-2.5 py-1.5 text-sm font-medium text-red-700 transition-colors hover:bg-red-100 dark:text-red-400 dark:hover:bg-red-900/30"
            aria-label="Permanently delete"
          >
            <AlertTriangle className="h-4 w-4" aria-hidden="true" />
            <span className="hidden sm:inline">Permanent Delete</span>
          </button>
          {showPermanentConfirm && (
            <>
              <div
                className="fixed inset-0 z-10"
                onClick={() => setShowPermanentConfirm(false)}
                aria-hidden="true"
              />
              <div className="absolute left-0 top-full z-20 mt-1 w-64 rounded-md border border-red-200 bg-white p-3 shadow-lg dark:border-red-800 dark:bg-gray-800">
                <p className="mb-2 text-sm text-gray-700 dark:text-gray-300">
                  Are you sure? This cannot be undone.
                </p>
                <div className="flex gap-2">
                  <button
                    type="button"
                    onClick={() => {
                      onPermanentDelete();
                      setShowPermanentConfirm(false);
                    }}
                    className="rounded-md bg-red-600 px-3 py-1 text-xs font-medium text-white hover:bg-red-700"
                  >
                    Delete permanently
                  </button>
                  <button
                    type="button"
                    onClick={() => setShowPermanentConfirm(false)}
                    className="rounded-md bg-gray-100 px-3 py-1 text-xs font-medium text-gray-700 hover:bg-gray-200 dark:bg-gray-700 dark:text-gray-300"
                  >
                    Cancel
                  </button>
                </div>
              </div>
            </>
          )}
        </div>
      )}

      {/* Standard Delete (move to trash) -- visible when NOT in trash */}
      {!isTrashView && (
        <button
          type="button"
          onClick={onDelete}
          className="flex items-center gap-1 rounded-md px-2.5 py-1.5 text-sm text-red-600 transition-colors hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-900/20"
          aria-label="Delete"
        >
          <Trash2 className="h-4 w-4" aria-hidden="true" />
          <span className="hidden sm:inline">Delete</span>
        </button>
      )}
    </div>
  );
}
