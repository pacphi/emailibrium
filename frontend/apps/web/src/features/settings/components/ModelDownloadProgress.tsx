import { useState, useCallback, useEffect, useRef } from 'react';

interface ModelDownloadProgressProps {
  modelId: string;
}

type DownloadStatus = 'idle' | 'checking' | 'downloading' | 'ready' | 'error';

interface ModelStatusResponse {
  modelId: string;
  status: 'cached' | 'downloading' | 'not_cached';
  cached: boolean;
}

export function ModelDownloadProgress({ modelId }: ModelDownloadProgressProps) {
  const [status, setStatus] = useState<DownloadStatus>('checking');
  const [errorMessage, setErrorMessage] = useState('');
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Clean up polling on unmount or modelId change.
  const stopPolling = useCallback(() => {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
  }, []);

  // Start polling model-status until cached.
  const startPolling = useCallback(() => {
    stopPolling();
    let polls = 0;
    const maxPolls = 300; // 10 minutes at 2s intervals
    pollRef.current = setInterval(async () => {
      polls++;
      if (polls > maxPolls) {
        stopPolling();
        setErrorMessage('Download timed out');
        setStatus('error');
        return;
      }
      try {
        const res = await fetch(`/api/v1/ai/model-status/${modelId}`);
        if (!res.ok) return;
        const data: ModelStatusResponse = await res.json();
        if (data.cached) {
          stopPolling();
          setStatus('ready');
        }
      } catch {
        // Retry on next tick
      }
    }, 2000);
  }, [modelId, stopPolling]);

  // Check model status on mount and when modelId changes.
  useEffect(() => {
    if (!modelId) {
      setStatus('idle');
      return;
    }

    setStatus('checking');
    stopPolling();

    fetch(`/api/v1/ai/model-status/${modelId}`, { signal: AbortSignal.timeout(3000) })
      .then((res) => {
        if (!res.ok) throw new Error(`Status check failed: ${res.status}`);
        return res.json() as Promise<ModelStatusResponse>;
      })
      .then((data) => {
        if (data.cached) {
          setStatus('ready');
        } else if (data.status === 'downloading') {
          setStatus('downloading');
          startPolling();
        } else {
          setStatus('idle');
        }
      })
      .catch(() => {
        setStatus('idle');
      });

    return () => {
      stopPolling();
    };
  }, [modelId, startPolling, stopPolling]);

  const handleDownload = useCallback(async () => {
    if (!modelId) return;
    setStatus('downloading');
    setErrorMessage('');
    try {
      const res = await fetch('/api/v1/ai/switch-model', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ modelId }),
      });
      if (!res.ok) throw new Error(`Download request failed: ${res.status}`);
      const data = await res.json();
      if (data.status === 'ready') {
        setStatus('ready');
        return;
      }
      // Start polling for completion.
      startPolling();
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Download failed');
      setStatus('error');
    }
  }, [modelId, startPolling]);

  if (!modelId) {
    return null;
  }

  return (
    <div className="space-y-2">
      <div className="text-xs text-gray-500 dark:text-gray-400">Model: {modelId}</div>

      {status === 'checking' && (
        <p className="text-xs text-gray-500 dark:text-gray-400">Checking model status...</p>
      )}

      {status === 'idle' && (
        <button
          type="button"
          onClick={handleDownload}
          className="px-3 py-1.5 rounded-lg border border-indigo-300 bg-indigo-50 text-indigo-700
            text-sm font-medium hover:bg-indigo-100 transition-colors
            dark:border-indigo-600 dark:bg-indigo-900/20 dark:text-indigo-300 dark:hover:bg-indigo-900/40"
        >
          Download Now
        </button>
      )}

      {status === 'downloading' && (
        <div className="space-y-1">
          <div className="relative h-2 w-full overflow-hidden rounded-full bg-gray-200 dark:bg-gray-700">
            <div className="h-full bg-indigo-600 dark:bg-indigo-400 animate-pulse w-full" />
          </div>
          <p className="text-xs text-gray-600 dark:text-gray-400">
            Downloading model... This may take a few minutes.
          </p>
        </div>
      )}

      {status === 'ready' && (
        <p className="text-xs text-green-600 dark:text-green-400 flex items-center gap-1">
          <span aria-hidden="true">&#10003;</span> Model ready
        </p>
      )}

      {status === 'error' && (
        <div className="space-y-1">
          <p className="text-xs text-red-600 dark:text-red-400">{errorMessage}</p>
          <button
            type="button"
            onClick={handleDownload}
            className="px-3 py-1.5 rounded-lg border border-red-300 text-red-700 text-sm font-medium
              hover:bg-red-50 transition-colors
              dark:border-red-700 dark:text-red-400 dark:hover:bg-red-900/20"
          >
            Retry
          </button>
        </div>
      )}
    </div>
  );
}
