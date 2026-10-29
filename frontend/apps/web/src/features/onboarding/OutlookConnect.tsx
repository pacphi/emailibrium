import { useState, useEffect, useRef } from 'react';
import type { DeviceCodeStart, EmailAccount } from '@emailibrium/types';
import { getAccounts, pollOutlookDeviceCode, startOutlookDeviceCode } from '@emailibrium/api';

interface OutlookConnectProps {
  onBack: () => void;
  onConnected: (account: EmailAccount) => void;
}

const SCOPES = [
  { label: 'Mail.Read', description: 'Read your mail' },
  { label: 'Mail.Send', description: 'Send mail on your behalf' },
  { label: 'Mail.ReadWrite', description: 'Manage folders and labels' },
  { label: 'offline_access', description: 'Keep access while you are away' },
];

type CodePhase =
  | { kind: 'enter-email' }
  | { kind: 'starting' }
  | { kind: 'waiting'; flow: DeviceCodeStart; interval: number }
  | { kind: 'ended'; message: string };

function errorMessage(err: unknown, fallback: string): string {
  return err instanceof Error && err.message ? err.message : fallback;
}

export function OutlookConnect({ onBack, onConnected }: OutlookConnectProps) {
  const [isRedirecting, setIsRedirecting] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [email, setEmail] = useState('');
  const [phase, setPhase] = useState<CodePhase>({ kind: 'enter-email' });
  const [copied, setCopied] = useState(false);
  const onConnectedRef = useRef(onConnected);
  onConnectedRef.current = onConnected;

  // After the Advanced OAuth redirect returns, the URL carries a success
  // query param; fetch the newly connected account and pass it upstream.
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    if (params.get('provider') === 'outlook' && params.get('status') === 'connected') {
      getAccounts().then((accounts) => {
        const outlook = accounts.find((a) => a.provider === 'outlook');
        if (outlook) {
          onConnected(outlook);
        }
      });
    }
  }, [onConnected]);

  // Poll the code sign-in no faster than the server's interval.
  const flowId = phase.kind === 'waiting' ? phase.flow.flowId : null;
  const interval = phase.kind === 'waiting' ? phase.interval : 0;
  useEffect(() => {
    if (!flowId) return;
    let cancelled = false;
    const timer = setTimeout(async () => {
      try {
        const result = await pollOutlookDeviceCode(flowId);
        if (cancelled) return;
        switch (result.status) {
          case 'connected':
            if (result.account) onConnectedRef.current(result.account);
            break;
          case 'expired':
            setPhase({ kind: 'ended', message: 'The code expired before sign-in finished.' });
            break;
          case 'declined':
            setPhase({ kind: 'ended', message: 'Sign-in was declined at Microsoft.' });
            break;
          default:
            setPhase((p) =>
              p.kind === 'waiting' ? { ...p, interval: result.interval ?? p.interval } : p,
            );
        }
      } catch (err) {
        if (!cancelled) {
          setPhase({ kind: 'ended', message: errorMessage(err, 'Sign-in failed.') });
        }
      }
    }, interval * 1000);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
    // Re-arm after every poll: `phase` changes identity on each update.
  }, [flowId, interval, phase]);

  async function handleGetCode(event: React.FormEvent) {
    event.preventDefault();
    const address = email.trim();
    if (!/^[^\s@]+@[^\s@]+$/.test(address)) {
      setPhase({ kind: 'ended', message: 'Enter a valid email address.' });
      return;
    }
    setPhase({ kind: 'starting' });
    setCopied(false);
    try {
      const flow = await startOutlookDeviceCode(address);
      setPhase({ kind: 'waiting', flow, interval: flow.interval });
    } catch (err) {
      setPhase({ kind: 'ended', message: errorMessage(err, 'Could not start sign-in.') });
    }
  }

  async function handleCopy(code: string) {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }

  function handleRedirect() {
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
        <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
          Connect Outlook <span className="text-xs font-medium text-indigo-600">Beta</span>
        </h3>
        <p className="text-sm text-gray-500 dark:text-gray-400">
          Sign in at Microsoft with a one-time code. No Azure app registration needed.
        </p>
      </div>

      {(phase.kind === 'enter-email' || phase.kind === 'starting' || phase.kind === 'ended') && (
        <form onSubmit={handleGetCode} className="space-y-3" noValidate>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
            Outlook or Microsoft 365 address
            <input
              type="email"
              autoComplete="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="you@outlook.com"
              className="mt-1 block w-full rounded-lg border border-gray-300 px-3 py-2 text-sm
                dark:bg-gray-800 dark:border-gray-600 dark:text-gray-100"
            />
          </label>
          {phase.kind === 'ended' && (
            <p role="alert" className="text-sm text-red-600 dark:text-red-400">
              {phase.message}
            </p>
          )}
          <button
            type="submit"
            disabled={phase.kind === 'starting'}
            className="w-full px-4 py-3 rounded-lg bg-[#0078D4] text-white font-medium shadow-sm
              hover:bg-[#106EBE] disabled:opacity-60 disabled:cursor-not-allowed transition-colors"
          >
            {phase.kind === 'starting'
              ? 'Getting a code...'
              : phase.kind === 'ended'
                ? 'Get a new code'
                : 'Get sign-in code'}
          </button>
          <p className="text-xs text-gray-500 dark:text-gray-400">
            IMAP must be turned on for this mailbox in Outlook settings.
          </p>
        </form>
      )}

      {phase.kind === 'waiting' && (
        <div className="space-y-4">
          <ol className="list-decimal list-inside space-y-1 text-sm text-gray-700 dark:text-gray-300">
            <li>
              Open{' '}
              <a
                href={phase.flow.verificationUri}
                target="_blank"
                rel="noopener noreferrer"
                className="font-medium text-[#0078D4] underline"
              >
                {phase.flow.verificationUri}
              </a>
            </li>
            <li>Type this code:</li>
          </ol>
          <div className="flex items-center justify-center gap-3">
            <code
              aria-label="Sign-in code"
              className="text-3xl font-mono font-semibold tracking-widest text-gray-900 dark:text-gray-100"
            >
              {phase.flow.userCode}
            </code>
            <button
              type="button"
              onClick={() => handleCopy(phase.flow.userCode)}
              className="rounded-md border border-gray-300 px-2 py-1 text-xs text-gray-600 hover:bg-gray-50
                dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
            >
              {copied ? 'Copied' : 'Copy'}
            </button>
          </div>
          <p className="text-sm text-gray-700 dark:text-gray-300">
            3. Sign in as <span className="font-medium">{email.trim()}</span>. Microsoft&apos;s
            consent screen shows the name of the sign-in app emailibrium uses.
          </p>
          <p
            role="status"
            aria-live="polite"
            className="text-sm text-center text-gray-500 dark:text-gray-400"
          >
            Waiting for you to finish signing in... (code expires in{' '}
            {Math.round(phase.flow.expiresIn / 60)} min)
          </p>
        </div>
      )}

      <div className="border-t border-gray-200 pt-4 dark:border-gray-700">
        <button
          type="button"
          aria-expanded={showAdvanced}
          onClick={() => setShowAdvanced((v) => !v)}
          className="text-xs text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
        >
          Advanced: use your own Azure app registration
        </button>
        {showAdvanced && (
          <div className="mt-3 space-y-3">
            <div className="rounded-lg border border-gray-200 bg-gray-50 p-4 dark:bg-gray-800 dark:border-gray-700">
              <p className="text-xs font-medium text-gray-700 dark:text-gray-300 mb-3">
                We will request the following permissions:
              </p>
              <ul className="space-y-2">
                {SCOPES.map((scope) => (
                  <li key={scope.label} className="text-sm text-gray-600 dark:text-gray-300">
                    <span className="font-medium">{scope.label}</span> &mdash; {scope.description}
                  </li>
                ))}
              </ul>
            </div>
            <button
              type="button"
              onClick={handleRedirect}
              disabled={isRedirecting}
              className="w-full px-4 py-2 rounded-lg border border-[#0078D4] text-[#0078D4] text-sm font-medium
                hover:bg-blue-50 disabled:opacity-60 disabled:cursor-not-allowed dark:hover:bg-gray-800"
            >
              {isRedirecting ? 'Redirecting to Microsoft...' : 'Sign in with Microsoft (redirect)'}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
