import { useState } from 'react';
import { Loader2, Download, Trash2, Shield } from 'lucide-react';
import type { ConsentPurpose } from '@emailibrium/types';
import { useConsents, useRecordConsent, useDataExport, useDataErase } from './hooks/useConsent';

const CONSENT_PURPOSES: Array<{
  purpose: ConsentPurpose;
  label: string;
  description: string;
}> = [
  {
    purpose: 'email_sync',
    label: 'Email Synchronization',
    description: 'Allow syncing email data from connected accounts for analysis.',
  },
  {
    purpose: 'ai_processing',
    label: 'AI Processing',
    description: 'Allow AI models to process your email content for insights and rule suggestions.',
  },
  {
    purpose: 'analytics',
    label: 'Analytics',
    description: 'Allow usage analytics to improve the application experience.',
  },
  {
    purpose: 'data_sharing',
    label: 'Anonymized Data Sharing',
    description: 'Share anonymized usage patterns to improve AI models.',
  },
];

function ConsentToggle({
  purpose,
  label,
  description,
  granted,
  isUpdating,
  onToggle,
}: {
  purpose: ConsentPurpose;
  label: string;
  description: string;
  granted: boolean;
  isUpdating: boolean;
  onToggle: (purpose: ConsentPurpose, granted: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between py-3">
      <div className="pr-4">
        <span className="block text-sm font-medium text-gray-700 dark:text-gray-300">{label}</span>
        <span className="text-xs text-gray-500 dark:text-gray-400">{description}</span>
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={granted}
        disabled={isUpdating}
        onClick={() => onToggle(purpose, !granted)}
        className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2
          border-transparent transition-colors focus:outline-none focus:ring-2 focus:ring-indigo-500
          focus:ring-offset-2 disabled:opacity-50 ${
            granted ? 'bg-indigo-600' : 'bg-gray-200 dark:bg-gray-600'
          }`}
      >
        <span
          className={`pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white
            shadow ring-0 transition-transform ${granted ? 'translate-x-5' : 'translate-x-0'}`}
        />
      </button>
    </div>
  );
}

export function ConsentSettings() {
  const consentsQuery = useConsents();
  const recordMutation = useRecordConsent();
  const exportMutation = useDataExport();
  const eraseMutation = useDataErase();

  const [exportFormat, setExportFormat] = useState<'json' | 'csv'>('json');
  const [showEraseConfirm, setShowEraseConfirm] = useState(false);

  const consents = consentsQuery.data ?? [];

  function isGranted(purpose: ConsentPurpose): boolean {
    const consent = consents.find((c) => c.purpose === purpose);
    return consent?.granted ?? false;
  }

  function handleToggle(purpose: ConsentPurpose, granted: boolean) {
    recordMutation.mutate({ purpose, granted });
  }

  function handleExport() {
    exportMutation.mutate({
      format: exportFormat,
      includeEmails: true,
      includeVectors: true,
      includeRules: true,
    });
  }

  function handleErase() {
    eraseMutation.mutate(
      { confirm: true },
      {
        onSuccess: () => {
          setShowEraseConfirm(false);
          localStorage.clear();
          window.location.href = '/onboarding';
        },
      },
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <Shield className="h-5 w-5 text-indigo-500" aria-hidden="true" />
        <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100">
          GDPR Consent Management
        </h3>
      </div>

      <p className="text-sm text-gray-500 dark:text-gray-400">
        Manage how your data is processed. Changes take effect immediately. You can withdraw consent
        at any time.
      </p>

      {/* Consent toggles */}
      <div className="divide-y divide-gray-200 rounded-lg border border-gray-200 bg-white px-4 dark:divide-gray-700 dark:border-gray-700 dark:bg-gray-800">
        {consentsQuery.isLoading ? (
          <div className="flex items-center justify-center py-6">
            <Loader2 className="h-5 w-5 animate-spin text-gray-400" />
            <span className="ml-2 text-sm text-gray-500">Loading consent preferences...</span>
          </div>
        ) : (
          CONSENT_PURPOSES.map((cp) => (
            <ConsentToggle
              key={cp.purpose}
              purpose={cp.purpose}
              label={cp.label}
              description={cp.description}
              granted={isGranted(cp.purpose)}
              isUpdating={recordMutation.isPending}
              onToggle={handleToggle}
            />
          ))
        )}
      </div>

      {/* Consent history */}
      {consents.length > 0 && (
        <div className="space-y-2">
          <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300">Consent History</h4>
          <div className="max-h-40 overflow-y-auto rounded-lg border border-gray-200 dark:border-gray-700">
            <table className="min-w-full divide-y divide-gray-200 text-sm dark:divide-gray-700">
              <thead className="bg-gray-50 dark:bg-gray-800">
                <tr>
                  <th className="px-3 py-2 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">
                    Purpose
                  </th>
                  <th className="px-3 py-2 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">
                    Status
                  </th>
                  <th className="px-3 py-2 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">
                    Date
                  </th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 bg-white dark:divide-gray-700 dark:bg-gray-800">
                {consents.map((c) => (
                  <tr key={c.id}>
                    <td className="px-3 py-2 text-xs text-gray-700 dark:text-gray-300">
                      {c.purpose.replace(/_/g, ' ')}
                    </td>
                    <td className="px-3 py-2">
                      <span
                        className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${
                          c.granted
                            ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400'
                            : 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400'
                        }`}
                      >
                        {c.granted ? 'Granted' : 'Withdrawn'}
                      </span>
                    </td>
                    <td className="px-3 py-2 text-xs text-gray-500 dark:text-gray-400">
                      {new Date(c.grantedAt).toLocaleDateString()}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Data export */}
      <div className="space-y-3 border-t border-gray-200 pt-4 dark:border-gray-700">
        <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300">Export Your Data</h4>
        <p className="text-xs text-gray-500 dark:text-gray-400">
          Download a copy of all your data in your preferred format.
        </p>
        <div className="flex items-center gap-3">
          <select
            value={exportFormat}
            onChange={(e) => setExportFormat(e.target.value as 'json' | 'csv')}
            className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm dark:border-gray-600 dark:bg-gray-800 dark:text-gray-200"
          >
            <option value="json">JSON</option>
            <option value="csv">CSV</option>
          </select>
          <button
            type="button"
            onClick={handleExport}
            disabled={exportMutation.isPending}
            className="flex items-center gap-2 rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700 disabled:opacity-50"
          >
            {exportMutation.isPending ? (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
            ) : (
              <Download className="h-4 w-4" aria-hidden="true" />
            )}
            {exportMutation.isPending ? 'Exporting...' : 'Export Data'}
          </button>
        </div>
        {exportMutation.isSuccess && exportMutation.data.downloadUrl && (
          <div className="rounded-md border border-green-200 bg-green-50 px-3 py-2 text-sm text-green-700 dark:border-green-800 dark:bg-green-900/20 dark:text-green-400">
            Export ready.{' '}
            <a
              href={exportMutation.data.downloadUrl}
              className="font-medium underline"
              target="_blank"
              rel="noopener noreferrer"
            >
              Download
            </a>
          </div>
        )}
        {exportMutation.isSuccess && exportMutation.data.status === 'processing' && (
          <p className="text-xs text-amber-600 dark:text-amber-400">
            Export is being prepared. You will be notified when it is ready.
          </p>
        )}
      </div>

      {/* Data erasure */}
      <div className="space-y-3 border-t border-gray-200 pt-4 dark:border-gray-700">
        <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300">
          Erase All Data (Right to be Forgotten)
        </h4>
        <p className="text-xs text-gray-500 dark:text-gray-400">
          Permanently delete all your data from our servers. This action cannot be undone.
        </p>
        {!showEraseConfirm ? (
          <button
            type="button"
            onClick={() => setShowEraseConfirm(true)}
            className="flex items-center gap-2 rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-red-700"
          >
            <Trash2 className="h-4 w-4" aria-hidden="true" />
            Erase All Data
          </button>
        ) : (
          <div className="rounded-lg border border-red-200 bg-red-50 p-4 dark:border-red-800 dark:bg-red-900/20">
            <p className="mb-3 text-sm font-medium text-red-700 dark:text-red-400">
              Are you absolutely sure? This will permanently delete all your emails, vectors, rules,
              settings, and consent records. This cannot be undone.
            </p>
            <div className="flex gap-2">
              <button
                type="button"
                onClick={handleErase}
                disabled={eraseMutation.isPending}
                className="flex items-center gap-2 rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-red-700 disabled:opacity-50"
              >
                {eraseMutation.isPending ? (
                  <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                ) : (
                  <Trash2 className="h-4 w-4" aria-hidden="true" />
                )}
                {eraseMutation.isPending ? 'Erasing...' : 'Yes, Erase Everything'}
              </button>
              <button
                type="button"
                onClick={() => setShowEraseConfirm(false)}
                className="rounded-md border border-gray-300 px-4 py-2 text-sm font-medium text-gray-700 transition-colors hover:bg-gray-100 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
