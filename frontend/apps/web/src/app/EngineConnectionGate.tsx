import { useCallback, useEffect, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { connectEngine, getEngineSession, lockEngine } from '@emailibrium/api';
import { hydrateFromBackend } from '@/features/settings/hooks/useSettings';

type ConnectionState = 'checking' | 'locked' | 'connected' | 'offline';

export function EngineConnectionGate({ children }: { children: ReactNode }) {
  const [state, setState] = useState<ConnectionState>('checking');
  const [token, setToken] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const sessionGeneration = useRef(0);
  const lockPending = useRef(false);

  const checkSession = useCallback(async () => {
    if (lockPending.current) return;
    const generation = ++sessionGeneration.current;
    try {
      const connected = await getEngineSession();
      if (generation !== sessionGeneration.current) return;
      setState(connected ? 'connected' : 'locked');
      if (connected) void hydrateFromBackend();
      setError(null);
    } catch {
      if (generation !== sessionGeneration.current) return;
      setState('offline');
      setError('Cannot reach the local engine. Start it and retry the connection.');
    }
  }, []);

  useEffect(() => {
    // Retire the legacy persistent bearer field. A browser session uses only
    // the HttpOnly cookie; the unlock token is never persisted by this UI.
    localStorage.removeItem('auth_token');
    void checkSession();
    const recheck = () => {
      void checkSession();
    };
    const expired = () => {
      void checkSession();
    };
    window.addEventListener('focus', recheck);
    window.addEventListener('emailibrium:unauthorized', expired);
    const timer = window.setInterval(recheck, 60_000);
    return () => {
      sessionGeneration.current += 1;
      window.removeEventListener('focus', recheck);
      window.removeEventListener('emailibrium:unauthorized', expired);
      window.clearInterval(timer);
    };
  }, [checkSession]);

  async function connect(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await connectEngine(token);
      setToken('');
      await checkSession();
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : 'Cannot reach the local engine. Retry the connection.',
      );
    } finally {
      setBusy(false);
    }
  }

  async function lock() {
    sessionGeneration.current += 1;
    lockPending.current = true;
    setBusy(true);
    try {
      await lockEngine();
      setState('locked');
      setError(null);
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : 'Unable to revoke the engine session. Retry locking.',
      );
    } finally {
      lockPending.current = false;
      setBusy(false);
    }
  }

  if (state === 'connected')
    return (
      <>
        {children}
        <div className="fixed bottom-3 right-3 z-40 flex items-center gap-2 rounded-lg bg-white p-2 shadow dark:bg-gray-800">
          {error && (
            <p role="alert" className="max-w-sm text-xs text-red-600">
              {error}
            </p>
          )}
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              void lock();
            }}
            className="text-xs text-gray-600 dark:text-gray-300"
          >
            Lock engine
          </button>
        </div>
      </>
    );

  return (
    <main className="flex min-h-screen items-center justify-center bg-gray-50 p-6 dark:bg-gray-900">
      <section className="w-full max-w-md space-y-5 rounded-xl bg-white p-8 shadow dark:bg-gray-800">
        <h1 className="text-2xl font-semibold text-gray-900 dark:text-white">
          Connect to your engine
        </h1>
        <p className="text-sm text-gray-600 dark:text-gray-300">
          Your mailbox stays locked until this browser has a verified local engine session.
        </p>
        {state === 'checking' && <p role="status">Checking your engine session…</p>}
        {error && (
          <p role="alert" className="text-sm text-red-600 dark:text-red-400">
            {error}
          </p>
        )}
        {state !== 'checking' && (
          <form
            onSubmit={(event) => {
              void connect(event);
            }}
            className="space-y-3"
          >
            <label
              htmlFor="engine-access-token"
              className="block text-sm font-medium dark:text-white"
            >
              Local access token
            </label>
            <input
              id="engine-access-token"
              type="password"
              autoComplete="off"
              value={token}
              onChange={(event) => setToken(event.target.value)}
              className="w-full rounded border p-2 dark:bg-gray-900 dark:text-white"
            />
            <p className="text-xs text-gray-500">
              Use the access token generated during local setup. It is never saved in browser
              storage. Sessions last up to eight hours and end when the engine restarts.
            </p>
            <button
              type="submit"
              disabled={busy || !token.trim()}
              className="rounded bg-indigo-600 px-4 py-2 text-white disabled:opacity-50"
            >
              {busy ? 'Connecting…' : 'Connect securely'}
            </button>
          </form>
        )}
        {state === 'offline' && (
          <button
            type="button"
            onClick={() => {
              void checkSession();
            }}
            className="text-sm text-indigo-600"
          >
            Retry connection
          </button>
        )}
      </section>
    </main>
  );
}
