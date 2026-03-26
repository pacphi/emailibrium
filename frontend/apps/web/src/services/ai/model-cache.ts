/**
 * BL-2.03 — Local model cache manager.
 *
 * Manages the on-disk cache directory where downloaded GGUF models are stored.
 * Cache location: ~/.emailibrium/models/llm/
 *
 * Node builtins are dynamically imported to avoid requiring @types/node at
 * compile time — consistent with the rest of this frontend-first codebase.
 */

import type { ModelManifest } from './model-manifest';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface CachedModel {
  modelId: string;
  path: string;
  sizeBytes: number;
  downloadedAt: Date;
}

/** Minimal stat result used by this module. */
interface FileStat {
  size: number;
  mtime: Date;
  isDirectory(): boolean;
}

// ---------------------------------------------------------------------------
// Node helpers (dynamically imported)
// ---------------------------------------------------------------------------

/* eslint-disable @typescript-eslint/no-explicit-any */
async function nodeFs(): Promise<{
  access: (p: string) => Promise<void>;
  mkdir: (p: string, o: { recursive: boolean }) => Promise<any>;
  readdir: (p: string) => Promise<string[]>;
  rm: (p: string, o: { recursive: boolean; force: boolean }) => Promise<void>;
  stat: (p: string) => Promise<FileStat>;
}> {
  return (await import('fs/promises')) as any;
}
/* eslint-enable @typescript-eslint/no-explicit-any */

async function nodePath(): Promise<{ join: (...parts: string[]) => string }> {
  return import('path');
}

async function nodeOs(): Promise<{ homedir: () => string }> {
  return import('os');
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Returns the absolute path to the model cache directory.
 */
export async function getCacheDir(): Promise<string> {
  const path = await nodePath();
  const os = await nodeOs();
  return path.join(os.homedir(), '.emailibrium', 'models', 'llm');
}

/**
 * Creates the cache directory tree if it does not already exist.
 *
 * @returns The absolute path to the cache directory.
 */
export async function ensureCacheDir(): Promise<string> {
  const dir = await getCacheDir();
  const fs = await nodeFs();
  await fs.mkdir(dir, { recursive: true });
  return dir;
}

/**
 * Checks whether a model is already present in the local cache.
 *
 * @param manifest - The model manifest to look up.
 */
export async function isModelCached(manifest: ModelManifest): Promise<boolean> {
  const filePath = await buildModelPath(manifest);
  return fileExists(filePath);
}

/**
 * Returns the full filesystem path to a cached model, or `null` if
 * the model has not been downloaded yet.
 *
 * @param manifest - The model manifest to look up.
 */
export async function getModelPath(manifest: ModelManifest): Promise<string | null> {
  const filePath = await buildModelPath(manifest);
  return (await fileExists(filePath)) ? filePath : null;
}

/**
 * Lists all models currently stored in the cache directory.
 */
export async function listCachedModels(): Promise<CachedModel[]> {
  const cacheDir = await getCacheDir();
  const fs = await nodeFs();
  const path = await nodePath();

  let entries: string[];
  try {
    entries = await fs.readdir(cacheDir);
  } catch {
    return [];
  }

  const models: CachedModel[] = [];

  for (const entry of entries) {
    const modelDir = path.join(cacheDir, entry);
    const dirStat = await safeStat(modelDir);
    if (!dirStat?.isDirectory()) continue;

    let files: string[];
    try {
      files = await fs.readdir(modelDir);
    } catch {
      continue;
    }

    for (const file of files) {
      if (!file.endsWith('.gguf')) continue;

      const filePath = path.join(modelDir, file);
      const fileStat = await safeStat(filePath);
      if (!fileStat) continue;

      models.push({
        modelId: entry,
        path: filePath,
        sizeBytes: fileStat.size,
        downloadedAt: fileStat.mtime,
      });
    }
  }

  return models;
}

/**
 * Removes a model and its directory from the cache.
 *
 * @param modelId - The model identifier to delete.
 */
export async function deleteModel(modelId: string): Promise<void> {
  const cacheDir = await getCacheDir();
  const path = await nodePath();
  const fs = await nodeFs();
  const modelDir = path.join(cacheDir, modelId);
  await fs.rm(modelDir, { recursive: true, force: true });
}

/**
 * Returns a disk-usage report for the model cache.
 */
export async function getCacheSize(): Promise<{
  totalBytes: number;
  models: { id: string; bytes: number }[];
}> {
  const cached = await listCachedModels();
  const models = cached.map((m) => ({ id: m.modelId, bytes: m.sizeBytes }));
  const totalBytes = models.reduce((sum, m) => sum + m.bytes, 0);
  return { totalBytes, models };
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

async function buildModelPath(manifest: ModelManifest): Promise<string> {
  const cacheDir = await getCacheDir();
  const path = await nodePath();
  return path.join(cacheDir, manifest.modelId, manifest.filename);
}

async function fileExists(filePath: string): Promise<boolean> {
  const fs = await nodeFs();
  try {
    await fs.access(filePath);
    return true;
  } catch {
    return false;
  }
}

async function safeStat(filePath: string): Promise<FileStat | null> {
  const fs = await nodeFs();
  try {
    return await fs.stat(filePath);
  } catch {
    return null;
  }
}
