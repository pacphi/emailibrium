import type { EmailAccount, Provider } from '@emailibrium/types';

function providerIcon(provider: Provider): React.ReactNode {
  switch (provider) {
    case 'gmail':
      return (
        <svg viewBox="0 0 24 24" className="w-5 h-5" aria-hidden="true">
          <path
            d="M24 5.457v13.909c0 .904-.732 1.636-1.636 1.636h-3.819V11.73L12 16.64l-6.545-4.91v9.273H1.636A1.636 1.636 0 010 19.366V5.457c0-2.023 2.309-3.178 3.927-1.964L12 9.545l8.073-6.052C21.691 2.279 24 3.434 24 5.457z"
            fill="#EA4335"
          />
        </svg>
      );
    case 'outlook':
      return (
        <svg viewBox="0 0 24 24" className="w-5 h-5" aria-hidden="true">
          <path
            d="M24 7.387v10.478c0 .23-.08.424-.238.576a.806.806 0 01-.587.234h-8.55v-6.9l1.675 1.238a.39.39 0 00.462 0l6.975-5.088a.272.272 0 01.263.012.244.244 0 01.137.213z"
            fill="#0078D4"
          />
        </svg>
      );
    default:
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.5}
          className="w-5 h-5 text-gray-500"
          aria-hidden="true"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            d="M21.75 6.75v10.5a2.25 2.25 0 01-2.25 2.25h-15a2.25 2.25 0 01-2.25-2.25V6.75m19.5 0A2.25 2.25 0 0019.5 4.5h-15a2.25 2.25 0 00-2.25 2.25m19.5 0v.243a2.25 2.25 0 01-1.07 1.916l-7.5 4.615a2.25 2.25 0 01-2.36 0L3.32 8.91a2.25 2.25 0 01-1.07-1.916V6.75"
          />
        </svg>
      );
  }
}

function statusBadge(account: EmailAccount): React.ReactNode {
  if (!account.isActive) {
    return (
      <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400">
        Inactive
      </span>
    );
  }
  if (account.lastSyncAt) {
    return (
      <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400">
        <span className="w-1.5 h-1.5 rounded-full bg-green-500" />
        Active
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400">
      <svg className="animate-spin w-3 h-3" viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <circle
          className="opacity-25"
          cx="12"
          cy="12"
          r="10"
          stroke="currentColor"
          strokeWidth="4"
        />
        <path
          className="opacity-75"
          fill="currentColor"
          d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
        />
      </svg>
      Syncing
    </span>
  );
}

interface ConnectedAccountsProps {
  accounts: EmailAccount[];
  onAddAnother: () => void;
  onContinue: () => void;
  onDisconnect?: (accountId: string) => void;
  onEdit?: (accountId: string) => void;
  showContinue?: boolean;
}

export function ConnectedAccounts({
  accounts,
  onAddAnother,
  onContinue,
  onDisconnect,
  onEdit,
  showContinue = true,
}: ConnectedAccountsProps) {
  return (
    <div className="max-w-lg mx-auto space-y-6">
      <div className="text-center space-y-1">
        <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
          Connected Accounts
        </h3>
        <p className="text-sm text-gray-500 dark:text-gray-400">
          {accounts.length === 0
            ? 'No accounts connected yet.'
            : `${accounts.length} account${accounts.length !== 1 ? 's' : ''} connected.`}
        </p>
      </div>

      {accounts.length > 0 && (
        <ul className="divide-y divide-gray-200 border border-gray-200 rounded-lg overflow-hidden dark:divide-gray-700 dark:border-gray-700">
          {accounts.map((account) => (
            <li
              key={account.id}
              className="flex items-center gap-3 px-4 py-3 bg-white dark:bg-gray-800"
            >
              {providerIcon(account.provider)}
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                  {account.emailAddress}
                </p>
                <p className="text-xs text-gray-500 dark:text-gray-400">
                  {account.emailCount.toLocaleString()} emails
                </p>
              </div>
              {statusBadge(account)}
              <div className="flex items-center gap-1">
                {onEdit && (
                  <button
                    type="button"
                    onClick={() => onEdit(account.id)}
                    className="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
                    aria-label={`Edit ${account.emailAddress}`}
                  >
                    <svg
                      viewBox="0 0 20 20"
                      fill="currentColor"
                      className="w-4 h-4"
                      aria-hidden="true"
                    >
                      <path d="M2.695 14.763l-1.262 3.154a.5.5 0 00.65.65l3.155-1.262a4 4 0 001.343-.885L17.5 5.5a2.121 2.121 0 00-3-3L3.58 13.42a4 4 0 00-.885 1.343z" />
                    </svg>
                  </button>
                )}
                {onDisconnect && (
                  <button
                    type="button"
                    onClick={() => onDisconnect(account.id)}
                    className="p-1 text-gray-400 hover:text-red-500 dark:hover:text-red-400 transition-colors"
                    aria-label={`Disconnect ${account.emailAddress}`}
                  >
                    <svg
                      viewBox="0 0 20 20"
                      fill="currentColor"
                      className="w-4 h-4"
                      aria-hidden="true"
                    >
                      <path
                        fillRule="evenodd"
                        d="M8.75 1A2.75 2.75 0 006 3.75v.443c-.795.077-1.584.176-2.365.298a.75.75 0 10.23 1.482l.149-.022.841 10.518A2.75 2.75 0 007.596 19h4.807a2.75 2.75 0 002.742-2.53l.841-10.519.149.023a.75.75 0 00.23-1.482A41.03 41.03 0 0014 4.193V3.75A2.75 2.75 0 0011.25 1h-2.5zM10 4c.84 0 1.673.025 2.5.075V3.75c0-.69-.56-1.25-1.25-1.25h-2.5c-.69 0-1.25.56-1.25 1.25v.325C8.327 4.025 9.16 4 10 4zM8.58 7.72a.75.75 0 00-1.5.06l.3 7.5a.75.75 0 101.5-.06l-.3-7.5zm4.34.06a.75.75 0 10-1.5-.06l-.3 7.5a.75.75 0 101.5.06l.3-7.5z"
                        clipRule="evenodd"
                      />
                    </svg>
                  </button>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}

      <div className="flex gap-3">
        <button
          type="button"
          onClick={onAddAnother}
          className="flex-1 px-4 py-2 rounded-lg border border-gray-300 text-gray-700 text-sm font-medium
            hover:bg-gray-50 transition-colors dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
        >
          Add Another Account
        </button>
        {showContinue && accounts.length > 0 && (
          <button
            type="button"
            onClick={onContinue}
            className="flex-1 px-4 py-2 rounded-lg bg-indigo-600 text-white text-sm font-medium
              hover:bg-indigo-700 transition-colors"
          >
            Continue
          </button>
        )}
      </div>
    </div>
  );
}
