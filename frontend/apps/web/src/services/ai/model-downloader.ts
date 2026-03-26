/**
 * BL-2.02 — GGUF model downloader using node-llama-cpp's HuggingFace Hub API.
 *
 * Supports progress reporting, cancellation via AbortSignal, and
 * deduplication of concurrent downloads for the same model.
 */

import type { ModelManifest } from './model-manifest';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface DownloadProgress {
  modelId: string;
  bytesDownloaded: number;
  totalBytes: number;
  percent: number;
}

export type DownloadProgressCallback = (progress: DownloadProgress) => void;

export interface DownloadOptions {
  onProgress?: DownloadProgressCallback;
  signal?: AbortSignal;
}

// ---------------------------------------------------------------------------
// In-flight download deduplication
// ---------------------------------------------------------------------------

const inFlightDownloads = new Map<string, Promise<string>>();

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Download a GGUF model from HuggingFace Hub to the specified directory.
 *
 * Concurrent calls for the same `modelId` are deduplicated — only one
 * network download runs, and all callers receive the same promise.
 *
 * @param manifest - Model manifest describing the file to download.
 * @param destDir  - Local directory to store the downloaded file.
 * @param options  - Optional progress callback and AbortSignal.
 * @returns The absolute path to the downloaded model file.
 */
export async function downloadModel(
  manifest: ModelManifest,
  destDir: string,
  options?: DownloadOptions,
): Promise<string> {
  const existing = inFlightDownloads.get(manifest.modelId);
  if (existing) return existing;

  const downloadPromise = executeDownload(manifest, destDir, options);

  inFlightDownloads.set(manifest.modelId, downloadPromise);

  try {
    const result = await downloadPromise;
    return result;
  } finally {
    inFlightDownloads.delete(manifest.modelId);
  }
}

/**
 * Cancel an in-flight download for the given model.
 * This removes the tracked promise; actual cancellation should be driven
 * by the AbortSignal passed in DownloadOptions.
 *
 * @param modelId - The model identifier to cancel.
 */
export function cancelDownload(modelId: string): void {
  inFlightDownloads.delete(modelId);
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

async function executeDownload(
  manifest: ModelManifest,
  destDir: string,
  options?: DownloadOptions,
): Promise<string> {
  const { createModelDownloader } = await import('node-llama-cpp');

  const modelUri = `hf:${manifest.repo}/${manifest.filename}`;

  const downloader = await createModelDownloader({
    modelUri,
    dirPath: destDir,
    onProgress: ({ downloadedSize, totalSize }) => {
      if (!options?.onProgress) return;

      const percent = totalSize > 0 ? Math.round((downloadedSize / totalSize) * 10_000) / 100 : 0;

      options.onProgress({
        modelId: manifest.modelId,
        bytesDownloaded: downloadedSize,
        totalBytes: totalSize,
        percent,
      });
    },
  });

  // Support cancellation via AbortSignal
  if (options?.signal) {
    options.signal.addEventListener(
      'abort',
      () => {
        downloader.cancel?.();
      },
      { once: true },
    );

    if (options.signal.aborted) {
      downloader.cancel?.();
      throw new Error(`Download aborted for model ${manifest.modelId}`);
    }
  }

  const modelPath: string = await downloader.download();
  return modelPath;
}
