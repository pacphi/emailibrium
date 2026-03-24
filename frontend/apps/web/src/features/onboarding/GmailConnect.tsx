import { useState } from 'react';

interface GmailConnectProps {
  onBack: () => void;
}

const SCOPES = [
  { label: 'Read', description: 'View your email messages' },
  { label: 'Send', description: 'Send email on your behalf' },
  { label: 'Label', description: 'Manage your labels' },
  { label: 'Archive', description: 'Archive and organize messages' },
];

export function GmailConnect({ onBack }: GmailConnectProps) {
  const [isRedirecting, setIsRedirecting] = useState(false);

  function handleConnect() {
    setIsRedirecting(true);
    window.location.href = '/api/v1/auth/gmail/connect';
  }

  return (
    <div className="max-w-md mx-auto space-y-6">
      <button
        type="button"
        onClick={onBack}
        className="flex items-center gap-1 text-sm text-gray-500 hover:text-gray-700 dark:text-gray-400
          dark:hover:text-gray-200 transition-colors"
      >
        <svg viewBox="0 0 20 20" fill="currentColor" className="w-4 h-4" aria-hidden="true">
          <path
            fillRule="evenodd"
            d="M17 10a.75.75 0 01-.75.75H5.612l4.158 3.96a.75.75 0 11-1.04 1.08l-5.5-5.25a.75.75 0 010-1.08l5.5-5.25a.75.75 0 111.04 1.08L5.612 9.25H16.25A.75.75 0 0117 10z"
            clipRule="evenodd"
          />
        </svg>
        Back to providers
      </button>

      <div className="text-center space-y-2">
        <svg viewBox="0 0 24 24" className="w-12 h-12 mx-auto" aria-hidden="true">
          <path
            d="M24 5.457v13.909c0 .904-.732 1.636-1.636 1.636h-3.819V11.73L12 16.64l-6.545-4.91v9.273H1.636A1.636 1.636 0 010 19.366V5.457c0-2.023 2.309-3.178 3.927-1.964L12 9.545l8.073-6.052C21.691 2.279 24 3.434 24 5.457z"
            fill="#EA4335"
          />
        </svg>
        <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">Connect Gmail</h3>
        <p className="text-sm text-gray-500 dark:text-gray-400">
          Sign in with your Google account to connect Gmail.
        </p>
      </div>

      <div className="rounded-lg border border-gray-200 bg-gray-50 p-4 dark:bg-gray-800 dark:border-gray-700">
        <p className="text-xs font-medium text-gray-700 dark:text-gray-300 mb-3">
          We will request the following permissions:
        </p>
        <ul className="space-y-2">
          {SCOPES.map((scope) => (
            <li key={scope.label} className="flex items-start gap-2 text-sm">
              <svg
                viewBox="0 0 20 20"
                fill="currentColor"
                className="w-4 h-4 mt-0.5 text-green-500 shrink-0"
                aria-hidden="true"
              >
                <path
                  fillRule="evenodd"
                  d="M16.704 4.153a.75.75 0 01.143 1.052l-8 10.5a.75.75 0 01-1.127.075l-4.5-4.5a.75.75 0 011.06-1.06l3.894 3.893 7.48-9.817a.75.75 0 011.05-.143z"
                  clipRule="evenodd"
                />
              </svg>
              <span className="text-gray-600 dark:text-gray-300">
                <span className="font-medium">{scope.label}</span> &mdash; {scope.description}
              </span>
            </li>
          ))}
        </ul>
      </div>

      <button
        type="button"
        onClick={handleConnect}
        disabled={isRedirecting}
        className="w-full flex items-center justify-center gap-2 px-4 py-3 rounded-lg bg-white border
          border-gray-300 text-gray-700 font-medium shadow-sm hover:bg-gray-50 disabled:opacity-60
          disabled:cursor-not-allowed transition-colors dark:bg-gray-700 dark:border-gray-600
          dark:text-gray-200 dark:hover:bg-gray-600"
      >
        {isRedirecting ? (
          <>
            <svg
              className="animate-spin h-5 w-5 text-gray-500"
              viewBox="0 0 24 24"
              fill="none"
              aria-hidden="true"
            >
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
            Redirecting to Google...
          </>
        ) : (
          <>
            <svg viewBox="0 0 24 24" className="w-5 h-5" aria-hidden="true">
              <path
                d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 01-2.2 3.32v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.1z"
                fill="#4285F4"
              />
              <path
                d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"
                fill="#34A853"
              />
              <path
                d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z"
                fill="#FBBC05"
              />
              <path
                d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"
                fill="#EA4335"
              />
            </svg>
            Sign in with Google
          </>
        )}
      </button>

      <p className="text-xs text-center text-gray-400 dark:text-gray-500">
        Your credentials are handled securely via OAuth 2.0. We never see your password.
      </p>
    </div>
  );
}
