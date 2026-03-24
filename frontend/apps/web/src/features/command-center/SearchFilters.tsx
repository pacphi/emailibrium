import { useState, useCallback } from 'react';
import type { SearchFilters as SearchFiltersType } from '@emailibrium/types';

const CATEGORIES = [
  'Work',
  'Personal',
  'Finance',
  'Social',
  'Shopping',
  'Travel',
  'Newsletters',
  'Promotions',
] as const;

const MATCH_LOCATIONS = ['Body', 'Attachments', 'Images', 'URLs'] as const;

interface SearchFiltersProps {
  filters: SearchFiltersType;
  onChange: (filters: SearchFiltersType) => void;
}

export function SearchFilters({ filters, onChange }: SearchFiltersProps) {
  const [senderInput, setSenderInput] = useState('');

  const updateFilter = useCallback(
    <K extends keyof SearchFiltersType>(key: K, value: SearchFiltersType[K]) => {
      onChange({ ...filters, [key]: value });
    },
    [filters, onChange],
  );

  const toggleCategory = useCallback(
    (category: string) => {
      const current = filters.categories ?? [];
      const next = current.includes(category)
        ? current.filter((c) => c !== category)
        : [...current, category];
      updateFilter('categories', next.length > 0 ? next : undefined);
    },
    [filters.categories, updateFilter],
  );

  const addSender = useCallback(() => {
    const trimmed = senderInput.trim();
    if (!trimmed) return;
    const current = filters.senders ?? [];
    if (!current.includes(trimmed)) {
      updateFilter('senders', [...current, trimmed]);
    }
    setSenderInput('');
  }, [senderInput, filters.senders, updateFilter]);

  const removeSender = useCallback(
    (sender: string) => {
      const next = (filters.senders ?? []).filter((s) => s !== sender);
      updateFilter('senders', next.length > 0 ? next : undefined);
    },
    [filters.senders, updateFilter],
  );

  const clearAll = useCallback(() => {
    onChange({});
  }, [onChange]);

  const hasActiveFilters =
    filters.dateFrom ||
    filters.dateTo ||
    (filters.senders && filters.senders.length > 0) ||
    (filters.categories && filters.categories.length > 0) ||
    filters.hasAttachment !== undefined ||
    filters.isRead !== undefined;

  return (
    <aside
      className="w-full space-y-5 rounded-xl border border-gray-200 bg-white p-4 dark:border-gray-700 dark:bg-gray-800 lg:w-64"
      aria-label="Search filters"
    >
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-gray-900 dark:text-white">Filters</h3>
        {hasActiveFilters && (
          <button
            type="button"
            onClick={clearAll}
            className="text-xs text-indigo-600 hover:text-indigo-800 dark:text-indigo-400 dark:hover:text-indigo-300"
          >
            Clear all
          </button>
        )}
      </div>

      {/* Date range */}
      <fieldset>
        <legend className="mb-1.5 text-xs font-medium text-gray-500 dark:text-gray-400">
          Date Range
        </legend>
        <div className="space-y-2">
          <label className="block">
            <span className="sr-only">From date</span>
            <input
              type="date"
              value={filters.dateFrom ?? ''}
              onChange={(e) => updateFilter('dateFrom', e.target.value || undefined)}
              className="w-full rounded-lg border border-gray-200 bg-white px-3 py-1.5 text-sm text-gray-900 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500 dark:border-gray-600 dark:bg-gray-700 dark:text-white"
              aria-label="From date"
            />
          </label>
          <label className="block">
            <span className="sr-only">To date</span>
            <input
              type="date"
              value={filters.dateTo ?? ''}
              onChange={(e) => updateFilter('dateTo', e.target.value || undefined)}
              className="w-full rounded-lg border border-gray-200 bg-white px-3 py-1.5 text-sm text-gray-900 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500 dark:border-gray-600 dark:bg-gray-700 dark:text-white"
              aria-label="To date"
            />
          </label>
        </div>
      </fieldset>

      {/* Sender filter */}
      <fieldset>
        <legend className="mb-1.5 text-xs font-medium text-gray-500 dark:text-gray-400">
          Sender
        </legend>
        <div className="flex gap-1">
          <input
            type="text"
            value={senderInput}
            onChange={(e) => setSenderInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                addSender();
              }
            }}
            placeholder="Enter sender..."
            className="min-w-0 flex-1 rounded-lg border border-gray-200 bg-white px-3 py-1.5 text-sm text-gray-900 placeholder-gray-400 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500 dark:border-gray-600 dark:bg-gray-700 dark:text-white dark:placeholder-gray-500"
            aria-label="Filter by sender"
          />
          <button
            type="button"
            onClick={addSender}
            className="shrink-0 rounded-lg bg-indigo-600 px-2 py-1.5 text-xs font-medium text-white hover:bg-indigo-700 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:ring-offset-2"
            aria-label="Add sender filter"
          >
            Add
          </button>
        </div>
        {filters.senders && filters.senders.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-1">
            {filters.senders.map((sender) => (
              <span
                key={sender}
                className="inline-flex items-center gap-1 rounded-full bg-indigo-50 px-2 py-0.5 text-xs text-indigo-700 dark:bg-indigo-900/30 dark:text-indigo-300"
              >
                {sender}
                <button
                  type="button"
                  onClick={() => removeSender(sender)}
                  className="text-indigo-400 hover:text-indigo-600 dark:text-indigo-500 dark:hover:text-indigo-300"
                  aria-label={`Remove sender ${sender}`}
                >
                  x
                </button>
              </span>
            ))}
          </div>
        )}
      </fieldset>

      {/* Category checkboxes */}
      <fieldset>
        <legend className="mb-1.5 text-xs font-medium text-gray-500 dark:text-gray-400">
          Category
        </legend>
        <div className="space-y-1.5">
          {CATEGORIES.map((category) => {
            const isChecked = filters.categories?.includes(category) ?? false;
            return (
              <label
                key={category}
                className="flex items-center gap-2 text-sm text-gray-700 dark:text-gray-300"
              >
                <input
                  type="checkbox"
                  checked={isChecked}
                  onChange={() => toggleCategory(category)}
                  className="h-3.5 w-3.5 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500 dark:border-gray-600"
                />
                {category}
              </label>
            );
          })}
        </div>
      </fieldset>

      {/* Has attachment toggle */}
      <fieldset>
        <legend className="mb-1.5 text-xs font-medium text-gray-500 dark:text-gray-400">
          Options
        </legend>
        <div className="space-y-2">
          <label className="flex items-center justify-between text-sm text-gray-700 dark:text-gray-300">
            <span>Has attachment</span>
            <ToggleSwitch
              checked={filters.hasAttachment ?? false}
              onChange={(checked) => updateFilter('hasAttachment', checked || undefined)}
              label="Has attachment"
            />
          </label>
          <label className="flex items-center justify-between text-sm text-gray-700 dark:text-gray-300">
            <span>Unread only</span>
            <ToggleSwitch
              checked={filters.isRead === false}
              onChange={(checked) => updateFilter('isRead', checked ? false : undefined)}
              label="Unread only"
            />
          </label>
        </div>
      </fieldset>

      {/* Match location (informational, for future extension) */}
      <fieldset>
        <legend className="mb-1.5 text-xs font-medium text-gray-500 dark:text-gray-400">
          Match Location
        </legend>
        <div className="space-y-1.5">
          {MATCH_LOCATIONS.map((location) => (
            <label
              key={location}
              className="flex items-center gap-2 text-sm text-gray-700 dark:text-gray-300"
            >
              <input
                type="checkbox"
                defaultChecked={location === 'Body'}
                className="h-3.5 w-3.5 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500 dark:border-gray-600"
              />
              {location}
            </label>
          ))}
        </div>
      </fieldset>
    </aside>
  );
}

/* Simple toggle switch component */

interface ToggleSwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
}

function ToggleSwitch({ checked, onChange, label }: ToggleSwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full transition-colors focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:ring-offset-2 ${
        checked ? 'bg-indigo-600' : 'bg-gray-200 dark:bg-gray-600'
      }`}
    >
      <span
        className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white shadow transition-transform ${
          checked ? 'translate-x-4' : 'translate-x-0.5'
        }`}
      />
    </button>
  );
}
