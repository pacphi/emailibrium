import { useSettings } from './hooks/useSettings';
import { api } from '@emailibrium/api';
import { useState } from 'react';

const SYNC_FREQUENCY_OPTIONS = [
  { value: 1, label: 'Every minute' },
  { value: 5, label: 'Every 5 minutes' },
  { value: 15, label: 'Every 15 minutes' },
  { value: 30, label: 'Every 30 minutes' },
  { value: 60, label: 'Every hour' },
];

export function GeneralSettings() {
  const {
    defaultComposeAccountId,
    notificationsEnabled,
    syncFrequencyMinutes,
    setDefaultComposeAccountId,
    setNotificationsEnabled,
    setSyncFrequencyMinutes,
  } = useSettings();

  const [isExporting, setIsExporting] = useState(false);

  async function handleExportData() {
    setIsExporting(true);
    try {
      const blob = await api.get('export').blob();
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `emailibrium-export-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch {
      // Silently fail for now; in production show a toast notification.
    } finally {
      setIsExporting(false);
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100 mb-4">
          General Settings
        </h3>
      </div>

      {/* Default compose account */}
      <div className="space-y-1">
        <label
          htmlFor="default-compose"
          className="block text-sm font-medium text-gray-700 dark:text-gray-300"
        >
          Default Compose Account
        </label>
        <input
          id="default-compose"
          type="text"
          value={defaultComposeAccountId ?? ''}
          onChange={(e) => setDefaultComposeAccountId(e.target.value || null)}
          placeholder="Select an account ID"
          className="w-full max-w-sm rounded-lg border border-gray-300 px-3 py-2 text-sm
            focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
            dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
        />
        <p className="text-xs text-gray-500 dark:text-gray-400">
          The account used by default when composing new emails.
        </p>
      </div>

      {/* Notifications */}
      <div className="flex items-center justify-between max-w-sm">
        <div>
          <span className="block text-sm font-medium text-gray-700 dark:text-gray-300">
            Notifications
          </span>
          <span className="text-xs text-gray-500 dark:text-gray-400">
            Show desktop and in-app notifications
          </span>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={notificationsEnabled}
          onClick={() => setNotificationsEnabled(!notificationsEnabled)}
          className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2
            border-transparent transition-colors focus:outline-none focus:ring-2 focus:ring-indigo-500
            focus:ring-offset-2 ${
              notificationsEnabled ? 'bg-indigo-600' : 'bg-gray-200 dark:bg-gray-600'
            }`}
        >
          <span
            className={`pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white
              shadow ring-0 transition-transform ${
                notificationsEnabled ? 'translate-x-5' : 'translate-x-0'
              }`}
          />
        </button>
      </div>

      {/* Sync frequency */}
      <div className="space-y-1">
        <label
          htmlFor="sync-frequency"
          className="block text-sm font-medium text-gray-700 dark:text-gray-300"
        >
          Sync Frequency
        </label>
        <select
          id="sync-frequency"
          value={syncFrequencyMinutes}
          onChange={(e) => setSyncFrequencyMinutes(Number(e.target.value))}
          className="w-full max-w-sm rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm
            focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
            dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
        >
          {SYNC_FREQUENCY_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
      </div>

      {/* Data export */}
      <div className="pt-4 border-t border-gray-200 dark:border-gray-700">
        <button
          type="button"
          onClick={handleExportData}
          disabled={isExporting}
          className="px-4 py-2 rounded-lg border border-gray-300 text-gray-700 text-sm font-medium
            hover:bg-gray-50 disabled:opacity-60 disabled:cursor-not-allowed transition-colors
            dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
        >
          {isExporting ? 'Exporting...' : 'Export All Data'}
        </button>
        <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
          Download all your local data as a JSON file.
        </p>
      </div>
    </div>
  );
}
