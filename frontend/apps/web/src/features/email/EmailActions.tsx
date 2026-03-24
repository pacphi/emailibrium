import { useState } from 'react';
import { Archive, Star, Tag, FolderInput, Trash2, ChevronDown } from 'lucide-react';

interface EmailActionsProps {
  emailId: string | null;
  selectedCount: number;
  onArchive: () => void;
  onStar: () => void;
  onDelete: () => void;
  onReclassify: (category: string) => void;
  onMove: (groupId: string) => void;
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
  onArchive,
  onStar,
  onDelete,
  onReclassify,
  onMove,
}: EmailActionsProps) {
  const hasTarget = emailId !== null || selectedCount > 0;
  const targetLabel = selectedCount > 1 ? `${selectedCount} emails` : 'email';

  if (!hasTarget) return null;

  return (
    <div
      className="flex items-center gap-1 border-b border-gray-200 bg-white px-3 py-2 dark:border-gray-700 dark:bg-gray-800"
      role="toolbar"
      aria-label={`Actions for ${targetLabel}`}
    >
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

      <div className="mx-1 h-5 w-px bg-gray-200 dark:bg-gray-600" aria-hidden="true" />

      <button
        type="button"
        onClick={onDelete}
        className="flex items-center gap-1 rounded-md px-2.5 py-1.5 text-sm text-red-600 transition-colors hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-900/20"
        aria-label="Delete"
      >
        <Trash2 className="h-4 w-4" aria-hidden="true" />
        <span className="hidden sm:inline">Delete</span>
      </button>
    </div>
  );
}
