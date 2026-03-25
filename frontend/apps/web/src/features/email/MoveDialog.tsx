import { useState, useRef, useEffect, useMemo } from 'react';
import { X, FolderInput, Search, Plus, Tag, Inbox, Mail } from 'lucide-react';
import type { FolderOrLabel } from '@emailibrium/api';

interface MoveDialogProps {
  isOpen: boolean;
  emailSubject: string;
  labels: FolderOrLabel[];
  onMove: (targetId: string, kind: 'folder' | 'label') => void;
  onCreateLabel?: (name: string) => void;
  onClose: () => void;
}

export function MoveDialog({
  isOpen,
  emailSubject,
  labels,
  onMove,
  onCreateLabel,
  onClose,
}: MoveDialogProps) {
  const [search, setSearch] = useState('');
  const [newLabel, setNewLabel] = useState('');
  const [showCreate, setShowCreate] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);

  // Focus search on open.
  useEffect(() => {
    if (isOpen) {
      setSearch('');
      setNewLabel('');
      setShowCreate(false);
      setTimeout(() => searchRef.current?.focus(), 100);
    }
  }, [isOpen]);

  // Close on Escape.
  useEffect(() => {
    if (!isOpen) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose();
    }
    document.addEventListener('keydown', handleKey);
    return () => document.removeEventListener('keydown', handleKey);
  }, [isOpen, onClose]);

  // Close on backdrop click.
  function handleBackdropClick(e: React.MouseEvent) {
    if (dialogRef.current && !dialogRef.current.contains(e.target as Node)) {
      onClose();
    }
  }

  const systemFolders = useMemo(
    () =>
      labels
        .filter((l) => l.isSystem && l.kind === 'folder')
        .filter(
          (l) =>
            !search || l.name.toLowerCase().includes(search.toLowerCase()),
        ),
    [labels, search],
  );

  const customLabels = useMemo(
    () =>
      labels
        .filter((l) => !l.isSystem)
        .filter(
          (l) =>
            !search || l.name.toLowerCase().includes(search.toLowerCase()),
        ),
    [labels, search],
  );

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 backdrop-blur-sm"
      onClick={handleBackdropClick}
      role="dialog"
      aria-modal="true"
      aria-label="Move email to folder"
    >
      <div
        ref={dialogRef}
        className="w-full max-w-lg rounded-xl bg-white shadow-2xl dark:bg-gray-800"
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-gray-200 px-4 py-3 dark:border-gray-700">
          <div className="flex items-center gap-2">
            <FolderInput className="h-5 w-5 text-indigo-600 dark:text-indigo-400" />
            <h2 className="text-sm font-semibold text-gray-900 dark:text-white">
              Move to folder
            </h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600 dark:hover:bg-gray-700"
            aria-label="Close"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Email subject preview */}
        <div className="flex items-center gap-2 border-b border-gray-100 px-4 py-2 dark:border-gray-700">
          <Mail className="h-3.5 w-3.5 shrink-0 text-gray-400 dark:text-gray-500" />
          <p className="truncate text-xs text-gray-500 dark:text-gray-400">
            {emailSubject}
          </p>
        </div>

        {/* Search */}
        <div className="border-b border-gray-100 px-4 py-2 dark:border-gray-700">
          <div className="flex items-center gap-2 rounded-lg border border-gray-200 bg-gray-50 px-3 py-1.5 dark:border-gray-600 dark:bg-gray-700">
            <Search className="h-4 w-4 text-gray-400" />
            <input
              ref={searchRef}
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search folders and labels..."
              className="flex-1 bg-transparent text-sm text-gray-900 outline-none placeholder:text-gray-400 dark:text-white"
            />
          </div>
        </div>

        {/* Folder/label list */}
        <div className="max-h-96 overflow-y-auto py-1">
          {/* System folders */}
          {systemFolders.length > 0 && (
            <div>
              <div className="px-4 py-1.5 text-xs font-semibold uppercase tracking-wider text-gray-400 dark:text-gray-500">
                Folders
              </div>
              {systemFolders.map((label) => (
                <button
                  key={label.id}
                  type="button"
                  onClick={() => {
                    onMove(label.id, label.kind);
                    onClose();
                  }}
                  className="flex w-full items-center gap-3 px-4 py-2 text-left text-sm text-gray-700 hover:bg-indigo-50 dark:text-gray-300 dark:hover:bg-indigo-900/20"
                >
                  <Inbox className="h-4 w-4 text-gray-400 dark:text-gray-500" />
                  {label.name}
                </button>
              ))}
            </div>
          )}

          {/* Custom labels */}
          {customLabels.length > 0 && (
            <div>
              {systemFolders.length > 0 && (
                <div className="my-1 border-t border-gray-100 dark:border-gray-700" />
              )}
              <div className="px-4 py-1.5 text-xs font-semibold uppercase tracking-wider text-gray-400 dark:text-gray-500">
                Labels
              </div>
              {customLabels.map((label) => (
                <button
                  key={label.id}
                  type="button"
                  onClick={() => {
                    onMove(label.id, label.kind);
                    onClose();
                  }}
                  className="flex w-full items-center gap-3 px-4 py-2 text-left text-sm text-gray-700 hover:bg-indigo-50 dark:text-gray-300 dark:hover:bg-indigo-900/20"
                >
                  <Tag className="h-4 w-4 text-gray-400 dark:text-gray-500" />
                  {label.name}
                </button>
              ))}
            </div>
          )}

          {/* Empty state */}
          {systemFolders.length === 0 && customLabels.length === 0 && (
            <div className="px-4 py-6 text-center text-sm text-gray-400">
              {search
                ? 'No matching folders or labels'
                : 'No folders available'}
            </div>
          )}
        </div>

        {/* Create new label */}
        <div className="border-t border-gray-200 px-4 py-3 dark:border-gray-700">
          {showCreate ? (
            <div className="flex items-center gap-2">
              <input
                type="text"
                value={newLabel}
                onChange={(e) => setNewLabel(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && newLabel.trim() && onCreateLabel) {
                    onCreateLabel(newLabel.trim());
                    setNewLabel('');
                    setShowCreate(false);
                  }
                }}
                placeholder="New label name..."
                className="flex-1 rounded-lg border border-gray-200 bg-white px-3 py-1.5 text-sm outline-none focus:border-indigo-500 dark:border-gray-600 dark:bg-gray-700 dark:text-white"
                autoFocus
              />
              <button
                type="button"
                onClick={() => {
                  if (newLabel.trim() && onCreateLabel) {
                    onCreateLabel(newLabel.trim());
                    setNewLabel('');
                    setShowCreate(false);
                  }
                }}
                disabled={!newLabel.trim()}
                className="rounded-lg bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
              >
                Create
              </button>
              <button
                type="button"
                onClick={() => setShowCreate(false)}
                className="text-sm text-gray-400 hover:text-gray-600"
              >
                Cancel
              </button>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setShowCreate(true)}
              className="flex items-center gap-2 text-sm font-medium text-indigo-600 hover:text-indigo-700 dark:text-indigo-400"
            >
              <Plus className="h-4 w-4" />
              Create new label
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
