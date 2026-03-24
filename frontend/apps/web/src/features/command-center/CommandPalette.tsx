import { useState, useCallback, useRef, useEffect } from 'react';
import { Command } from 'cmdk';
import { useCommandPalette } from './hooks/useCommandPalette';
import { useDebounce } from './hooks/useDebounce';
import { useSearch } from './hooks/useSearch';
import type { SearchMode } from '@emailibrium/types';

interface PaletteAction {
  id: string;
  label: string;
  shortcut?: string;
  onSelect: () => void;
}

const ACTIONS: PaletteAction[] = [
  {
    id: 'clean-inbox',
    label: 'Clean Inbox',
    shortcut: 'C',
    onSelect: () => navigateTo('/inbox-cleaner'),
  },
  {
    id: 'view-insights',
    label: 'View Insights',
    shortcut: 'I',
    onSelect: () => navigateTo('/insights'),
  },
  {
    id: 'manage-rules',
    label: 'Manage Rules',
    shortcut: 'R',
    onSelect: () => navigateTo('/rules'),
  },
  { id: 'settings', label: 'Settings', shortcut: 'S', onSelect: () => navigateTo('/settings') },
  { id: 'add-account', label: 'Add Account', onSelect: () => navigateTo('/settings') },
  { id: 'chat-ai', label: 'Chat with AI', onSelect: () => navigateTo('/chat') },
];

function navigateTo(path: string) {
  window.location.href = path;
}

export function CommandPalette() {
  const { isOpen, close } = useCommandPalette();
  const [inputValue, setInputValue] = useState('');
  const debouncedQuery = useDebounce(inputValue, 300);
  const inputRef = useRef<HTMLInputElement>(null);

  const searchQuery = {
    text: debouncedQuery,
    mode: 'hybrid' as SearchMode,
    limit: 8,
  };

  const { data: searchResponse, isLoading: isSearching } = useSearch(searchQuery);

  const handleSelect = useCallback(
    (value: string) => {
      const action = ACTIONS.find((a) => a.id === value);
      if (action) {
        action.onSelect();
        close();
        return;
      }

      // If it looks like an email result, navigate to the email view
      if (value.startsWith('email:')) {
        const emailId = value.replace('email:', '');
        navigateTo(`/email?id=${encodeURIComponent(emailId)}`);
        close();
      }
    },
    [close],
  );

  useEffect(() => {
    if (isOpen && inputRef.current) {
      inputRef.current.focus();
    }
    if (!isOpen) {
      setInputValue('');
    }
  }, [isOpen]);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center pt-[20vh]">
      {/* Backdrop */}
      <div
        className="fixed inset-0 bg-black/50 backdrop-blur-sm"
        onClick={close}
        aria-hidden="true"
      />

      {/* Palette container */}
      <div
        className="relative w-full max-w-xl"
        role="dialog"
        aria-label="Command palette"
        aria-modal="true"
      >
        <Command
          className="rounded-xl border border-gray-200 bg-white shadow-2xl dark:border-gray-700 dark:bg-gray-800"
          onKeyDown={(e: React.KeyboardEvent) => {
            if (e.key === 'Escape') {
              close();
            }
          }}
        >
          <div className="flex items-center border-b border-gray-200 px-4 dark:border-gray-700">
            <SearchIcon />
            <Command.Input
              ref={inputRef}
              value={inputValue}
              onValueChange={setInputValue}
              placeholder="Search emails, actions, topics..."
              className="flex-1 border-0 bg-transparent py-3 pl-2 text-sm text-gray-900 placeholder-gray-400 outline-none dark:text-white dark:placeholder-gray-500"
              aria-label="Search command palette"
            />
            {isSearching && (
              <div
                className="h-4 w-4 animate-spin rounded-full border-2 border-gray-300 border-t-indigo-600"
                aria-label="Searching"
              />
            )}
            <kbd className="ml-2 hidden rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-400 sm:inline-block dark:bg-gray-700 dark:text-gray-500">
              ESC
            </kbd>
          </div>

          <Command.List className="max-h-80 overflow-y-auto p-2">
            <Command.Empty className="px-4 py-8 text-center text-sm text-gray-500 dark:text-gray-400">
              No results found.
            </Command.Empty>

            {/* Email search results */}
            {searchResponse && searchResponse.results.length > 0 && (
              <Command.Group heading="Emails" className="mb-2">
                <p className="px-2 pb-1 text-xs font-medium text-gray-400 dark:text-gray-500">
                  Emails
                </p>
                {searchResponse.results.map((result) => (
                  <Command.Item
                    key={result.emailId}
                    value={`email:${result.emailId}`}
                    onSelect={handleSelect}
                    className="flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-sm text-gray-700 aria-selected:bg-indigo-50 aria-selected:text-indigo-900 dark:text-gray-300 dark:aria-selected:bg-indigo-900/30 dark:aria-selected:text-indigo-200"
                  >
                    <MailResultIcon />
                    <div className="min-w-0 flex-1">
                      <p className="truncate font-medium">
                        {result.metadata?.subject ?? `Email ${result.emailId}`}
                      </p>
                      <p className="truncate text-xs text-gray-400">
                        {result.metadata?.from ?? 'Unknown sender'} -- Score:{' '}
                        {Math.round(result.score * 100)}%
                      </p>
                    </div>
                    <span className="shrink-0 rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-500 dark:bg-gray-700 dark:text-gray-400">
                      {result.matchType}
                    </span>
                  </Command.Item>
                ))}
              </Command.Group>
            )}

            {/* Actions */}
            <Command.Group heading="Actions" className="mb-2">
              <p className="px-2 pb-1 text-xs font-medium text-gray-400 dark:text-gray-500">
                Actions
              </p>
              {ACTIONS.map((action) => (
                <Command.Item
                  key={action.id}
                  value={action.id}
                  onSelect={handleSelect}
                  className="flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-sm text-gray-700 aria-selected:bg-indigo-50 aria-selected:text-indigo-900 dark:text-gray-300 dark:aria-selected:bg-indigo-900/30 dark:aria-selected:text-indigo-200"
                >
                  <ActionIcon />
                  <span className="flex-1">{action.label}</span>
                  {action.shortcut && (
                    <kbd className="rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-400 dark:bg-gray-700 dark:text-gray-500">
                      {action.shortcut}
                    </kbd>
                  )}
                </Command.Item>
              ))}
            </Command.Group>
          </Command.List>

          {/* Footer */}
          <div className="flex items-center justify-between border-t border-gray-200 px-4 py-2 text-xs text-gray-400 dark:border-gray-700 dark:text-gray-500">
            <div className="flex gap-2">
              <span>
                <kbd className="rounded bg-gray-100 px-1 dark:bg-gray-700">&uarr;&darr;</kbd>{' '}
                Navigate
              </span>
              <span>
                <kbd className="rounded bg-gray-100 px-1 dark:bg-gray-700">&crarr;</kbd> Select
              </span>
            </div>
            {searchResponse && (
              <span>
                {searchResponse.total} result{searchResponse.total !== 1 ? 's' : ''} in{' '}
                {searchResponse.latencyMs}ms
              </span>
            )}
          </div>
        </Command>
      </div>
    </div>
  );
}

function SearchIcon() {
  return (
    <svg
      className="h-5 w-5 shrink-0 text-gray-400"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
      />
    </svg>
  );
}

function MailResultIcon() {
  return (
    <svg
      className="h-4 w-4 shrink-0 text-gray-400"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z"
      />
    </svg>
  );
}

function ActionIcon() {
  return (
    <svg
      className="h-4 w-4 shrink-0 text-gray-400"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path strokeLinecap="round" strokeLinejoin="round" d="M13 10V3L4 14h7v7l9-11h-7z" />
    </svg>
  );
}
