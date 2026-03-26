import { useState, useCallback } from 'react';
import * as Progress from '@radix-ui/react-progress';
import { getManifest, type ModelManifest } from '../../../services/ai/model-manifest';

interface ModelDownloadProgressProps {
  modelId: string;
}

type DownloadStatus = 'idle' | 'downloading' | 'ready' | 'error';

interface DownloadProgress {
  percent: number;
  bytesDownloaded: number;
  totalBytes: number;
}

function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(0)} MB`;
  return `${(bytes / 1e3).toFixed(0)} KB`;
}

/** Placeholder — will be wired to the real download manager in BL-3.05. */
async function startDownload(
  _manifest: ModelManifest,
  onProgress: (p: DownloadProgress) => void,
): Promise<void> {
  // Simulate download progress for UI development.
  // Replace with actual wasm-llm manager call in BL-3.05.
  const total = _manifest.sizeBytes;
  const steps = 20;
  for (let i = 1; i <= steps; i++) {
    await new Promise((r) => setTimeout(r, 150));
    onProgress({
      percent: (i / steps) * 100,
      bytesDownloaded: (total / steps) * i,
      totalBytes: total,
    });
  }
}

export function ModelDownloadProgress({ modelId }: ModelDownloadProgressProps) {
  const [status, setStatus] = useState<DownloadStatus>('idle');
  const [progress, setProgress] = useState<DownloadProgress>({
    percent: 0,
    bytesDownloaded: 0,
    totalBytes: 0,
  });
  const [errorMessage, setErrorMessage] = useState('');

  const manifest = getManifest(modelId);

  const handleDownload = useCallback(async () => {
    if (!manifest) return;
    setStatus('downloading');
    setErrorMessage('');
    setProgress({ percent: 0, bytesDownloaded: 0, totalBytes: manifest.sizeBytes });
    try {
      await startDownload(manifest, setProgress);
      setStatus('ready');
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Download failed');
      setStatus('error');
    }
  }, [manifest]);

  if (!manifest) {
    return <p className="text-xs text-red-600 dark:text-red-400">Unknown model: {modelId}</p>;
  }

  return (
    <div className="space-y-2">
      <div className="text-xs text-gray-500 dark:text-gray-400">
        {manifest.displayName} &middot; {formatBytes(manifest.sizeBytes)} download &middot;{' '}
        {formatBytes(manifest.ramEstimateBytes)} RAM
      </div>

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
          <Progress.Root
            value={progress.percent}
            max={100}
            className="relative h-2 w-full overflow-hidden rounded-full bg-gray-200 dark:bg-gray-700"
          >
            <Progress.Indicator
              className="h-full bg-indigo-600 transition-transform duration-200 dark:bg-indigo-400"
              style={{ width: `${progress.percent}%` }}
            />
          </Progress.Root>
          <p className="text-xs text-gray-600 dark:text-gray-400 tabular-nums">
            {progress.percent.toFixed(0)}% &middot; {formatBytes(progress.bytesDownloaded)} /{' '}
            {formatBytes(progress.totalBytes)}
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
