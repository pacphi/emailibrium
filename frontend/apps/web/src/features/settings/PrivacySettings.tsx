import { useState } from 'react';
import { useSettings } from './hooks/useSettings';

interface AuditLogEntry {
  id: string;
  timestamp: string;
  action: string;
  resource: string;
  details: string;
}

const DATA_RETENTION_OPTIONS = [
  { value: 30, label: '30 days' },
  { value: 60, label: '60 days' },
  { value: 90, label: '90 days' },
  { value: 180, label: '6 months' },
  { value: 365, label: '1 year' },
  { value: -1, label: 'Forever' },
];

export function PrivacySettings() {
  const { encryptionAtRest, dataRetentionDays, setEncryptionAtRest, setDataRetentionDays } =
    useSettings();

  const [showPasswordForm, setShowPasswordForm] = useState(false);
  const [masterPassword, setMasterPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [passwordError, setPasswordError] = useState<string | null>(null);
  const [isDeletingData, setIsDeletingData] = useState(false);

  // In production, audit log entries would come from an API/react-query call.
  const [auditLog] = useState<AuditLogEntry[]>([
    {
      id: '1',
      timestamp: new Date().toISOString(),
      action: 'vector_store_read',
      resource: 'embeddings/inbox',
      details: 'Search query executed',
    },
    {
      id: '2',
      timestamp: new Date(Date.now() - 3600000).toISOString(),
      action: 'vector_store_write',
      resource: 'embeddings/inbox',
      details: 'New embeddings indexed (42 emails)',
    },
  ]);

  function handleToggleEncryption() {
    if (!encryptionAtRest) {
      setShowPasswordForm(true);
    } else {
      setEncryptionAtRest(false);
      setShowPasswordForm(false);
    }
  }

  function handleSetPassword() {
    setPasswordError(null);
    if (masterPassword.length < 8) {
      setPasswordError('Password must be at least 8 characters');
      return;
    }
    if (masterPassword !== confirmPassword) {
      setPasswordError('Passwords do not match');
      return;
    }
    // In production, hash the password and store via secure storage API.
    setEncryptionAtRest(true);
    setShowPasswordForm(false);
    setMasterPassword('');
    setConfirmPassword('');
  }

  async function handleDeleteAllData() {
    if (
      !window.confirm('This will permanently delete all local data. This action cannot be undone.')
    ) {
      return;
    }
    setIsDeletingData(true);
    try {
      // In production, call the API to wipe local stores.
      await new Promise((resolve) => setTimeout(resolve, 1000));
      localStorage.clear();
      window.location.href = '/onboarding';
    } finally {
      setIsDeletingData(false);
    }
  }

  return (
    <div className="space-y-6">
      <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100">Privacy Settings</h3>

      {/* Encryption at rest */}
      <div className="space-y-3">
        <div className="flex items-center justify-between max-w-sm">
          <div>
            <span className="block text-sm font-medium text-gray-700 dark:text-gray-300">
              Encryption at Rest
            </span>
            <span className="text-xs text-gray-500 dark:text-gray-400">
              Encrypt local data with a master password
            </span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={encryptionAtRest}
            onClick={handleToggleEncryption}
            className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2
              border-transparent transition-colors focus:outline-none focus:ring-2 focus:ring-indigo-500
              focus:ring-offset-2 ${
                encryptionAtRest ? 'bg-indigo-600' : 'bg-gray-200 dark:bg-gray-600'
              }`}
          >
            <span
              className={`pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white
                shadow ring-0 transition-transform ${
                  encryptionAtRest ? 'translate-x-5' : 'translate-x-0'
                }`}
            />
          </button>
        </div>

        {showPasswordForm && (
          <div className="max-w-sm space-y-3 rounded-lg border border-gray-200 bg-gray-50 p-4 dark:bg-gray-800 dark:border-gray-700">
            <div>
              <label
                htmlFor="master-password"
                className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1"
              >
                Master Password
              </label>
              <input
                id="master-password"
                type="password"
                value={masterPassword}
                onChange={(e) => setMasterPassword(e.target.value)}
                className="w-full rounded border border-gray-300 px-3 py-2 text-sm
                  dark:bg-gray-700 dark:border-gray-600 dark:text-gray-200"
                placeholder="At least 8 characters"
              />
            </div>
            <div>
              <label
                htmlFor="confirm-password"
                className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1"
              >
                Confirm Password
              </label>
              <input
                id="confirm-password"
                type="password"
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
                className="w-full rounded border border-gray-300 px-3 py-2 text-sm
                  dark:bg-gray-700 dark:border-gray-600 dark:text-gray-200"
              />
            </div>
            {passwordError && <p className="text-xs text-red-600">{passwordError}</p>}
            <div className="flex gap-2">
              <button
                type="button"
                onClick={handleSetPassword}
                className="px-3 py-1.5 rounded text-xs font-medium bg-indigo-600 text-white hover:bg-indigo-700"
              >
                Set Password
              </button>
              <button
                type="button"
                onClick={() => {
                  setShowPasswordForm(false);
                  setMasterPassword('');
                  setConfirmPassword('');
                  setPasswordError(null);
                }}
                className="px-3 py-1.5 rounded text-xs font-medium border border-gray-300 text-gray-700
                  hover:bg-gray-100 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Audit log */}
      <div className="space-y-2">
        <span className="block text-sm font-medium text-gray-700 dark:text-gray-300">
          Audit Log
        </span>
        <p className="text-xs text-gray-500 dark:text-gray-400">
          Recent vector store accesses and modifications.
        </p>
        <div className="overflow-auto rounded-lg border border-gray-200 dark:border-gray-700">
          <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-700 text-sm">
            <thead className="bg-gray-50 dark:bg-gray-800">
              <tr>
                <th className="px-3 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                  Time
                </th>
                <th className="px-3 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                  Action
                </th>
                <th className="px-3 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                  Resource
                </th>
                <th className="px-3 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                  Details
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 dark:divide-gray-700 bg-white dark:bg-gray-800">
              {auditLog.map((entry) => (
                <tr key={entry.id}>
                  <td className="px-3 py-2 text-xs text-gray-600 dark:text-gray-400 whitespace-nowrap">
                    {new Date(entry.timestamp).toLocaleString()}
                  </td>
                  <td className="px-3 py-2 text-xs font-mono text-gray-700 dark:text-gray-300">
                    {entry.action}
                  </td>
                  <td className="px-3 py-2 text-xs text-gray-600 dark:text-gray-400">
                    {entry.resource}
                  </td>
                  <td className="px-3 py-2 text-xs text-gray-600 dark:text-gray-400">
                    {entry.details}
                  </td>
                </tr>
              ))}
              {auditLog.length === 0 && (
                <tr>
                  <td colSpan={4} className="px-3 py-4 text-center text-xs text-gray-500">
                    No audit log entries yet.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* Data retention */}
      <div className="space-y-1">
        <label
          htmlFor="data-retention"
          className="block text-sm font-medium text-gray-700 dark:text-gray-300"
        >
          Data Retention Period
        </label>
        <select
          id="data-retention"
          value={dataRetentionDays}
          onChange={(e) => setDataRetentionDays(Number(e.target.value))}
          className="w-full max-w-sm rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm
            focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500
            dark:bg-gray-800 dark:border-gray-600 dark:text-gray-200"
        >
          {DATA_RETENTION_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
        <p className="text-xs text-gray-500 dark:text-gray-400">
          How long to keep local email data and embeddings.
        </p>
      </div>

      {/* Delete all data */}
      <div className="pt-4 border-t border-gray-200 dark:border-gray-700">
        <button
          type="button"
          onClick={handleDeleteAllData}
          disabled={isDeletingData}
          className="px-4 py-2 rounded-lg bg-red-600 text-white text-sm font-medium
            hover:bg-red-700 disabled:opacity-60 disabled:cursor-not-allowed transition-colors"
        >
          {isDeletingData ? 'Deleting...' : 'Delete All Local Data'}
        </button>
        <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
          Permanently removes all local data, embeddings, and settings. This cannot be undone.
        </p>
      </div>
    </div>
  );
}
