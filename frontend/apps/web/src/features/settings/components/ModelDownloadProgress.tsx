import { useState, useCallback, useEffect } from 'react';

interface ModelDownloadProgressProps {
  modelId: string;
}

type DownloadStatus = 'idle' | 'checking' | 'downloading' | 'ready' | 'error';

interface DownloadProgress {
  percent: number;
  bytesDownloaded: number;
  totalBytes: number;
}

interface ModelStatusResponse {
  modelId: string;
  status: 'cached' | 'downloading' | 'not_cached';
  cached: boolean;
}

export function ModelDownloadProgress({ modelId }: ModelDownloadProgressProps) {
  const [status, setStatus] = useState<DownloadStatus>('checking');
  const [progress, setProgress] = useState<DownloadProgress>({
    percent: 0,
    bytesDownloaded: 0,
    totalBytes: 0,
  });
  const [errorMessage, setErrorMessage] = useState('');

  // Check model status from the backend API on mount and when modelId changes.
  useEffect(() => {
    if (!modelId) {
      setStatus('idle');
      return;
    }

    let cancelled = false;
    setStatus('checking');

    fetch(`/api/v1/ai/model-status/${modelId}`, { signal: AbortSignal.timeout(3000) })
      .then((res) => {
        if (!res.ok) throw new Error(`Status check failed: ${res.status}`);
        return res.json() as Promise<ModelStatusResponse>;
      })
      .then((data) => {
        if (cancelled) return;
        if (data.cached) {
          setStatus('ready');
        } else if (data.status === 'downloading') {
          setStatus('downloading');
        } else {
          setStatus('idle');
        }
      })
      .catch(() => {
        if (!cancelled) setStatus('idle');
      });

    return () => {
      cancelled = true;
    };
  }, [modelId]);

  const handleDownload = useCallback(async () => {
    if (!modelId) return;
    setStatus('downloading');
    setErrorMessage('');
    setProgress({ percent: 0, bytesDownloaded: 0, totalBytes: 0 });
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
      // Poll for download completion.
      let polls = 0;
      const maxPolls = 150;
      const poll = setInterval(async () => {
        polls++;
        if (polls > maxPolls) {
          clearInterval(poll);
          setErrorMessage('Download timed out');
          setStatus('error');
          return;
        }
        try {
          const statusRes = await fetch(`/api/v1/ai/model-status/${modelId}`);
          if (!statusRes.ok) return;
          const statusData: ModelStatusResponse = await statusRes.json();
          setProgress((prev) => ({
            ...prev,
            percent: Math.min((polls / maxPolls) * 100, 95),
          }));
          if (statusData.cached) {
            clearInterval(poll);
            setProgress({ percent: 100, bytesDownloaded: 0, totalBytes: 0 });
            setStatus('ready');
          }
        } catch {
          // Retry on next tick
        }
      }, 2000);
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Download failed');
      setStatus('error');
    }
  }, [modelId]);

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
            <div
              className="h-full bg-indigo-600 transition-all duration-200 dark:bg-indigo-400"
              style={{ width: `${progress.percent}%` }}
            />
          </div>
          {progress.percent > 0 && (
            <p className="text-xs text-gray-600 dark:text-gray-400 tabular-nums">
              {progress.percent.toFixed(0)}% downloading...
            </p>
          )}
          {progress.percent === 0 && (
            <p className="text-xs text-gray-600 dark:text-gray-400">Starting download...</p>
          )}
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
