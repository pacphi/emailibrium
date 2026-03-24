import { useState } from 'react';

interface OutlookConnectProps {
  onBack: () => void;
}

const SCOPES = [
  { label: 'Mail.Read', description: 'Read your mail' },
  { label: 'Mail.Send', description: 'Send mail on your behalf' },
  { label: 'Mail.ReadWrite', description: 'Manage folders and labels' },
  { label: 'offline_access', description: 'Keep access while you are away' },
];

export function OutlookConnect({ onBack }: OutlookConnectProps) {
  const [isRedirecting, setIsRedirecting] = useState(false);

  function handleConnect() {
    setIsRedirecting(true);
    window.location.href = '/api/v1/auth/outlook/connect';
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
            d="M24 7.387v10.478c0 .23-.08.424-.238.576a.806.806 0 01-.587.234h-8.55v-6.9l1.675 1.238a.39.39 0 00.462 0l6.975-5.088a.272.272 0 01.263.012.244.244 0 01.137.213zm0-1.588c0-.4-.288-.587-.863-.563l-7.512 5.475-1.675-1.238H9.375v13.15h13.8c.225 0 .413-.075.563-.225s.225-.338.225-.563V10.05L24 7.387zM14.625.6H1.2C.537.6 0 1.137 0 1.8v20.4c0 .663.537 1.2 1.2 1.2h13.425V.6z"
            fill="#0078D4"
          />
        </svg>
        <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">Connect Outlook</h3>
        <p className="text-sm text-gray-500 dark:text-gray-400">
          Sign in with your Microsoft account to connect Outlook.
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
        className="w-full flex items-center justify-center gap-2 px-4 py-3 rounded-lg bg-[#0078D4]
          text-white font-medium shadow-sm hover:bg-[#106EBE] disabled:opacity-60
          disabled:cursor-not-allowed transition-colors"
      >
        {isRedirecting ? (
          <>
            <svg
              className="animate-spin h-5 w-5 text-white"
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
            Redirecting to Microsoft...
          </>
        ) : (
          <>
            <svg viewBox="0 0 23 23" className="w-5 h-5" aria-hidden="true">
              <rect x="1" y="1" width="10" height="10" fill="#F25022" />
              <rect x="12" y="1" width="10" height="10" fill="#7FBA00" />
              <rect x="1" y="12" width="10" height="10" fill="#00A4EF" />
              <rect x="12" y="12" width="10" height="10" fill="#FFB900" />
            </svg>
            Sign in with Microsoft
          </>
        )}
      </button>

      <p className="text-xs text-center text-gray-400 dark:text-gray-500">
        Your credentials are handled securely via OAuth 2.0. We never see your password.
      </p>
    </div>
  );
}
