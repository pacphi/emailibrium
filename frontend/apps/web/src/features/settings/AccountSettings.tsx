import { useState, useCallback } from 'react';
import type { ArchiveStrategy, EmailAccount } from '@emailibrium/types';

const ARCHIVE_OPTIONS: { value: ArchiveStrategy; label: string }[] = [
  { value: 'instant', label: 'Instant' },
  { value: 'delayed', label: 'Delayed (60s)' },
  { value: 'manual', label: 'Manual' },
];

const SYNC_DEPTH_OPTIONS = [
  { value: '7d', label: '7 days' },
  { value: '30d', label: '30 days' },
  { value: '90d', label: '90 days' },
  { value: '365d', label: '1 year' },
  { value: 'all', label: 'All time' },
];

const SYNC_FREQ_OPTIONS = [
  { value: 1, label: '1 min' },
  { value: 5, label: '5 min' },
  { value: 15, label: '15 min' },
  { value: 60, label: '1 hour' },
];

interface AccountCardProps {
  account: EmailAccount;
  onUpdate: (id: string, changes: Partial<EmailAccount>) => void;
  onRemove: (id: string) => void;
}

function AccountCard({ account, onUpdate, onRemove }: AccountCardProps) {
  const [showDanger, setShowDanger] = useState(false);

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4 dark:bg-gray-800 dark:border-gray-700">
      <div className="flex items-center justify-between mb-4">
        <div>
          <p className="text-sm font-medium text-gray-900 dark:text-gray-100">
            {account.emailAddress}
          </p>
          <p className="text-xs text-gray-500 dark:text-gray-400 capitalize">
            {account.provider} &middot; {account.emailCount.toLocaleString()} emails
          </p>
        </div>
        <span
          className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium ${
            account.isActive
              ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400'
              : 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400'
          }`}
        >
          {account.isActive ? 'Active' : 'Inactive'}
        </span>
      </div>

      <div className="grid grid-cols-2 gap-4">
        {/* Archive strategy */}
        <div>
          <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
            Archive Strategy
          </label>
          <select
            value={account.archiveStrategy}
            onChange={(e) =>
              onUpdate(account.id, { archiveStrategy: e.target.value as ArchiveStrategy })
            }
            className="w-full rounded border border-gray-300 bg-white px-2 py-1.5 text-xs
              dark:bg-gray-700 dark:border-gray-600 dark:text-gray-200"
          >
            {ARCHIVE_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </div>

        {/* Sync frequency */}
        <div>
          <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
            Sync Frequency
          </label>
          <select
            defaultValue={5}
            className="w-full rounded border border-gray-300 bg-white px-2 py-1.5 text-xs
              dark:bg-gray-700 dark:border-gray-600 dark:text-gray-200"
          >
            {SYNC_FREQ_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </div>

        {/* Sync depth */}
        <div>
          <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
            Sync Depth
          </label>
          <select
            value={account.syncDepth}
            onChange={(e) => onUpdate(account.id, { syncDepth: e.target.value })}
            className="w-full rounded border border-gray-300 bg-white px-2 py-1.5 text-xs
              dark:bg-gray-700 dark:border-gray-600 dark:text-gray-200"
          >
            {SYNC_DEPTH_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </div>

        {/* Label prefix */}
        <div>
          <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
            Label Prefix
          </label>
          <input
            type="text"
            value={account.labelPrefix}
            onChange={(e) => onUpdate(account.id, { labelPrefix: e.target.value })}
            className="w-full rounded border border-gray-300 px-2 py-1.5 text-xs
              dark:bg-gray-700 dark:border-gray-600 dark:text-gray-200"
          />
        </div>
      </div>

      {/* Danger zone */}
      <div className="mt-4 pt-3 border-t border-gray-200 dark:border-gray-700">
        <button
          type="button"
          onClick={() => setShowDanger(!showDanger)}
          className="text-xs text-red-600 hover:text-red-700 dark:text-red-400 dark:hover:text-red-300 font-medium"
        >
          {showDanger ? 'Hide danger zone' : 'Danger zone...'}
        </button>
        {showDanger && (
          <div className="mt-3 flex flex-wrap gap-2">
            <button
              type="button"
              className="px-3 py-1.5 rounded text-xs font-medium border border-red-300 text-red-700
                hover:bg-red-50 dark:border-red-700 dark:text-red-400 dark:hover:bg-red-900/20"
            >
              Remove Labels
            </button>
            <button
              type="button"
              className="px-3 py-1.5 rounded text-xs font-medium border border-red-300 text-red-700
                hover:bg-red-50 dark:border-red-700 dark:text-red-400 dark:hover:bg-red-900/20"
            >
              Unarchive All
            </button>
            <button
              type="button"
              onClick={() => onRemove(account.id)}
              className="px-3 py-1.5 rounded text-xs font-medium bg-red-600 text-white
                hover:bg-red-700"
            >
              Disconnect &amp; Delete Local Data
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

export function AccountSettings() {
  // In production, accounts would come from the API via react-query.
  const [accounts, setAccounts] = useState<EmailAccount[]>([]);

  const handleUpdate = useCallback((id: string, changes: Partial<EmailAccount>) => {
    setAccounts((prev) => prev.map((a) => (a.id === id ? { ...a, ...changes } : a)));
  }, []);

  const handleRemove = useCallback((id: string) => {
    setAccounts((prev) => prev.filter((a) => a.id !== id));
  }, []);

  function handleAddAccount() {
    window.location.href = '/onboarding';
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100">
          Account Settings
        </h3>
        <button
          type="button"
          onClick={handleAddAccount}
          className="px-3 py-1.5 rounded-lg bg-indigo-600 text-white text-sm font-medium
            hover:bg-indigo-700 transition-colors"
        >
          Add Account
        </button>
      </div>

      {accounts.length === 0 ? (
        <div className="text-center py-12 text-gray-500 dark:text-gray-400">
          <p className="text-sm">No accounts connected.</p>
          <button
            type="button"
            onClick={handleAddAccount}
            className="mt-2 text-sm text-indigo-600 hover:text-indigo-700 dark:text-indigo-400 font-medium"
          >
            Connect your first account
          </button>
        </div>
      ) : (
        <div className="space-y-4">
          {accounts.map((account) => (
            <AccountCard
              key={account.id}
              account={account}
              onUpdate={handleUpdate}
              onRemove={handleRemove}
            />
          ))}
        </div>
      )}
    </div>
  );
}
