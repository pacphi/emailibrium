import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mocks — must be declared before any module imports that use them

const mockGetLlama = vi.fn();
const mockLlamaChatSession = vi.fn();
const mockCreateModelDownloader = vi.fn();

vi.mock('node-llama-cpp', () => ({
  getLlama: (...args: unknown[]) => mockGetLlama(...args),
  LlamaChatSession: mockLlamaChatSession,
  createModelDownloader: (...args: unknown[]) => mockCreateModelDownloader(...args),
}));

const mockStat = vi.fn();
const mockReaddir = vi.fn();
const mockMkdir = vi.fn();
const mockRm = vi.fn();
const mockAccess = vi.fn();

vi.mock('fs/promises', () => ({
  stat: (...args: unknown[]) => mockStat(...args),
  readdir: (...args: unknown[]) => mockReaddir(...args),
  mkdir: (...args: unknown[]) => mockMkdir(...args),
  rm: (...args: unknown[]) => mockRm(...args),
  access: (...args: unknown[]) => mockAccess(...args),
}));

const mockHomedir = vi.fn();
vi.mock('os', () => ({ homedir: () => mockHomedir() }));

function setupDefaultMocks(): void {
  mockHomedir.mockReturnValue('/home/testuser');
  mockMkdir.mockResolvedValue(undefined);
  mockRm.mockResolvedValue(undefined);
  mockAccess.mockResolvedValue(undefined);

  const mockModel = {
    dispose: vi.fn().mockResolvedValue(undefined),
    tokenize: vi.fn().mockReturnValue([1, 2, 3]),
    createContext: vi.fn().mockResolvedValue({
      getSequence: vi.fn().mockReturnValue({}),
      dispose: vi.fn(),
    }),
  };
  const mockLlama = {
    loadModel: vi.fn().mockResolvedValue(mockModel),
    createGrammarForJsonSchema: vi.fn().mockResolvedValue({}),
    getGpuDeviceNames: vi.fn().mockResolvedValue([]),
    getVramState: vi.fn().mockResolvedValue({ total: 0, used: 0, free: 0 }),
    dispose: vi.fn(),
  };
  mockGetLlama.mockResolvedValue(mockLlama);
  mockLlamaChatSession.mockImplementation(() => ({
    prompt: vi.fn().mockResolvedValue('{}'),
    dispose: vi.fn(),
  }));
  mockCreateModelDownloader.mockResolvedValue({
    download: vi.fn().mockResolvedValue('/home/testuser/.emailibrium/models/llm/test/model.gguf'),
    cancel: vi.fn(),
  });
}

// Imports — after mocks

import { getAllManifests, getManifest, getDefaultManifest } from '../model-manifest';
import {
  getCacheDir,
  isModelCached,
  getModelPath,
  deleteModel,
  getCacheSize,
} from '../model-cache';
import { downloadModel } from '../model-downloader';
import { BuiltInLlmManager } from '../built-in-llm-manager';

// Tests

describe('model-manifest', () => {
  it('getAllManifests() returns 5 models', () => {
    expect(getAllManifests()).toHaveLength(5);
  });

  it('getManifest() returns correct manifest by ID', () => {
    const m = getManifest('qwen2.5-0.5b-q4km');
    expect(m).toBeDefined();
    expect(m!.displayName).toBe('Qwen 2.5 0.5B Instruct');
    expect(m!.repo).toBe('Qwen/Qwen2.5-0.5B-Instruct-GGUF');
  });

  it('getManifest() returns undefined for unknown ID', () => {
    expect(getManifest('nonexistent-model')).toBeUndefined();
  });

  it('getDefaultManifest() returns qwen2.5-0.5b-q4km', () => {
    expect(getDefaultManifest().modelId).toBe('qwen2.5-0.5b-q4km');
  });

  it('default model has isDefault: true', () => {
    expect(getDefaultManifest().isDefault).toBe(true);
  });
});

describe('model-cache', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setupDefaultMocks();
  });

  it('getCacheDir() returns correct path', async () => {
    const dir = await getCacheDir();
    expect(dir).toContain('/home/testuser');
    expect(dir).toContain('.emailibrium');
  });

  it('isModelCached() returns true when file exists', async () => {
    mockAccess.mockResolvedValue(undefined);
    expect(await isModelCached(getManifest('qwen2.5-0.5b-q4km')!)).toBe(true);
  });

  it('isModelCached() returns false when file missing', async () => {
    mockAccess.mockRejectedValue(new Error('ENOENT'));
    expect(await isModelCached(getManifest('qwen2.5-0.5b-q4km')!)).toBe(false);
  });

  it('getModelPath() returns path when cached', async () => {
    mockAccess.mockResolvedValue(undefined);
    const path = await getModelPath(getManifest('qwen2.5-0.5b-q4km')!);
    expect(path).toContain('qwen2.5-0.5b-q4km');
    expect(path).toContain('.gguf');
  });

  it('getModelPath() returns null when not cached', async () => {
    mockAccess.mockRejectedValue(new Error('ENOENT'));
    expect(await getModelPath(getManifest('qwen2.5-0.5b-q4km')!)).toBeNull();
  });

  it('deleteModel() removes model directory', async () => {
    await deleteModel('qwen2.5-0.5b-q4km');
    expect(mockRm).toHaveBeenCalledWith(expect.stringContaining('qwen2.5-0.5b-q4km'), {
      recursive: true,
      force: true,
    });
  });

  it('getCacheSize() reports correct totals', async () => {
    mockReaddir.mockResolvedValueOnce(['model-a']).mockResolvedValueOnce(['file.gguf']);
    mockStat
      .mockResolvedValueOnce({ isDirectory: () => true, size: 0 })
      .mockResolvedValueOnce({ size: 500_000, mtime: new Date() });

    const { totalBytes, models } = await getCacheSize();
    expect(models).toHaveLength(1);
    expect(totalBytes).toBe(500_000);
    expect(models[0]!.id).toBe('model-a');
  });
});

describe('model-downloader', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setupDefaultMocks();
  });

  it('downloadModel() calls createModelDownloader with correct URI', async () => {
    const manifest = getManifest('qwen2.5-0.5b-q4km')!;
    await downloadModel(manifest, '/tmp/cache');
    expect(mockCreateModelDownloader).toHaveBeenCalledWith(
      expect.objectContaining({
        modelUri: `hf:${manifest.repo}/${manifest.filename}`,
        dirPath: '/tmp/cache',
      }),
    );
  });

  it('downloadModel() reports progress', async () => {
    let onProg: ((p: { downloadedSize: number; totalSize: number }) => void) | null = null;
    mockCreateModelDownloader.mockImplementation(async (opts: { onProgress?: typeof onProg }) => {
      onProg = opts.onProgress ?? null;
      return {
        download: vi.fn().mockImplementation(async () => {
          onProg?.({ downloadedSize: 100, totalSize: 200 });
          return '/tmp/model.gguf';
        }),
        cancel: vi.fn(),
      };
    });

    const cb = vi.fn();
    await downloadModel(getManifest('qwen2.5-0.5b-q4km')!, '/tmp/cache', { onProgress: cb });
    expect(cb).toHaveBeenCalledWith(
      expect.objectContaining({ modelId: 'qwen2.5-0.5b-q4km', percent: expect.any(Number) }),
    );
  });

  it('downloadModel() deduplicates concurrent downloads for same model', async () => {
    let resolve: ((v: string) => void) | null = null;
    mockCreateModelDownloader.mockImplementation(async () => ({
      download: vi.fn().mockImplementation(
        () =>
          new Promise<string>((r) => {
            resolve = r;
          }),
      ),
      cancel: vi.fn(),
    }));

    const manifest = getManifest('smollm2-360m-q4km')!;
    const p1 = downloadModel(manifest, '/tmp/cache');
    const p2 = downloadModel(manifest, '/tmp/cache');
    await new Promise((r) => setTimeout(r, 10));

    expect(mockCreateModelDownloader).toHaveBeenCalledTimes(1);
    resolve!('/tmp/model.gguf');
    const [r1, r2] = await Promise.all([p1, p2]);
    expect(r1).toBe(r2);
  });

  it('downloadModel() handles cancellation via AbortSignal', async () => {
    mockCreateModelDownloader.mockResolvedValue({
      download: vi.fn().mockResolvedValue('/tmp/model.gguf'),
      cancel: vi.fn(),
    });
    const controller = new AbortController();
    controller.abort();
    await expect(
      downloadModel(getManifest('llama3.2-3b-q4km')!, '/tmp/cache', { signal: controller.signal }),
    ).rejects.toThrow(/abort/i);
  });
});

describe('BuiltInLlmManager', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setupDefaultMocks();
  });

  it('initialize() downloads if not cached, then loads', async () => {
    let callCount = 0;
    mockAccess.mockImplementation(async () => {
      callCount++;
      if (callCount <= 1) throw new Error('ENOENT');
    });

    const manager = new BuiltInLlmManager();
    await manager.initialize();
    expect(manager.getStatus().isModelLoaded).toBe(true);
    expect(mockCreateModelDownloader).toHaveBeenCalled();
    await manager.shutdown();
  });

  it('initialize() skips download if cached', async () => {
    const manager = new BuiltInLlmManager();
    await manager.initialize();
    expect(manager.getStatus().isModelLoaded).toBe(true);
    expect(mockCreateModelDownloader).not.toHaveBeenCalled();
    await manager.shutdown();
  });

  it('classify() lazy-initializes then classifies', async () => {
    mockLlamaChatSession.mockImplementation(() => ({
      prompt: vi.fn().mockResolvedValue(JSON.stringify({ category: 'primary', confidence: 0.9 })),
      dispose: vi.fn(),
    }));

    const manager = new BuiltInLlmManager();
    const result = await manager.classify({
      subject: 'Test',
      sender: 'test@test.com',
      bodyPreview: 'Hello',
      categories: ['primary', 'social'],
    });
    expect(result.category).toBe('primary');
    expect(result.confidence).toBe(0.9);
    expect(manager.getStatus().isModelLoaded).toBe(true);
    await manager.shutdown();
  });

  it('switchModel() unloads old, downloads new, loads new', async () => {
    const manager = new BuiltInLlmManager();
    await manager.initialize();
    expect(manager.getStatus().modelId).toBe('qwen2.5-0.5b-q4km');

    await manager.switchModel('smollm2-360m-q4km');
    expect(manager.getStatus().modelId).toBe('smollm2-360m-q4km');
    expect(manager.getStatus().isModelLoaded).toBe(true);
    await manager.shutdown();
  });

  it('shutdown() unloads and clears state', async () => {
    const manager = new BuiltInLlmManager();
    await manager.initialize();
    expect(manager.getStatus().isModelLoaded).toBe(true);

    await manager.shutdown();
    const status = manager.getStatus();
    expect(status.isModelLoaded).toBe(false);
    expect(status.hardware).toBeNull();
    expect(status.downloadProgress).toBeNull();
  });

  it('getStatus() reflects current state', async () => {
    const manager = new BuiltInLlmManager();
    expect(manager.getStatus().isModelLoaded).toBe(false);
    expect(manager.getStatus().downloadProgress).toBeNull();

    await manager.initialize();
    expect(manager.getStatus().isModelLoaded).toBe(true);
    expect(manager.getStatus().modelId).toBe('qwen2.5-0.5b-q4km');
    expect(manager.getStatus().hardware).toBeDefined();
    await manager.shutdown();
  });
});
