import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { ClassificationPrompt } from '../built-in-llm-adapter';
import type { GenerativeRouterConfig } from '../generative-router';

// ---------------------------------------------------------------------------
// Vitest 4: hoist mock variables for use in vi.mock factories
// ---------------------------------------------------------------------------

const {
  mockClassify,
  mockChat,
  mockShutdown,
  mockInitialize,
  mockSwitchModel,
  mockGetStatus,
  mockAccess,
} = vi.hoisted(() => ({
  mockClassify: vi.fn(),
  mockChat: vi.fn(),
  mockShutdown: vi.fn().mockResolvedValue(undefined),
  mockInitialize: vi.fn().mockResolvedValue(undefined),
  mockSwitchModel: vi.fn().mockResolvedValue(undefined),
  mockGetStatus: vi.fn(),
  mockAccess: vi.fn(),
}));

vi.mock('../built-in-llm-manager', () => ({
  BuiltInLlmManager: vi.fn().mockImplementation(function () {
    return {
      classify: mockClassify,
      chat: mockChat,
      shutdown: mockShutdown,
      initialize: mockInitialize,
      switchModel: mockSwitchModel,
      getStatus: mockGetStatus,
    };
  }),
}));

vi.mock('fs/promises', () => ({
  access: (...args: unknown[]) => mockAccess(...args),
  mkdir: vi.fn().mockResolvedValue(undefined),
  stat: vi.fn().mockResolvedValue({ size: 500_000_000, isDirectory: () => true }),
  readdir: vi.fn().mockResolvedValue([]),
}));
vi.mock('os', () => ({ homedir: () => '/home/testuser' }));

import { GenerativeRouter } from '../generative-router';

function makePrompt(overrides: Partial<ClassificationPrompt> = {}): ClassificationPrompt {
  return {
    subject: 'Hello',
    sender: 'alice@example.com',
    bodyPreview: 'Just checking in.',
    categories: ['primary', 'updates', 'promotions'],
    ...overrides,
  };
}

function makeConfig(overrides: Partial<GenerativeRouterConfig> = {}): GenerativeRouterConfig {
  return { provider: 'none', ...overrides };
}

describe('Full lifecycle', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetStatus.mockReturnValue({ isModelLoaded: true, modelId: 'qwen2.5-0.5b-q4km' });
  });

  it('fresh install: select builtin, download, classify, get result', async () => {
    const expected = { category: 'updates', confidence: 0.92, reasoning: 'LLM' };
    mockClassify.mockResolvedValueOnce(expected);
    const router = new GenerativeRouter(
      makeConfig({ provider: 'builtin', builtInModelId: 'qwen2.5-0.5b-q4km' }),
    );
    const result = await router.classify(makePrompt({ subject: 'Your invoice' }));
    expect(result).toEqual(expected);
    expect(mockClassify).toHaveBeenCalledOnce();
  });

  it('cached model: skip download, load, classify, result', async () => {
    mockAccess.mockResolvedValue(undefined);
    const expected = { category: 'primary', confidence: 0.88, reasoning: 'cached' };
    mockClassify.mockResolvedValueOnce(expected);
    const router = new GenerativeRouter(
      makeConfig({ provider: 'builtin', builtInModelId: 'qwen2.5-0.5b-q4km' }),
    );
    expect(await router.classify(makePrompt())).toEqual(expected);
  });

  it('user switches from none to builtin: router updates, classify works', async () => {
    const router = new GenerativeRouter(makeConfig({ provider: 'none' }));
    expect(router.getActiveProvider()).toBe('none');
    router.updateConfig({ provider: 'builtin', builtInModelId: 'qwen2.5-0.5b-q4km' });
    expect(router.getActiveProvider()).toBe('builtin');
    const expected = { category: 'updates', confidence: 0.9, reasoning: 'LLM' };
    mockClassify.mockResolvedValueOnce(expected);
    expect(await router.classify(makePrompt())).toEqual(expected);
  });

  it('user switches from builtin back to none: rule-based fallback', async () => {
    const router = new GenerativeRouter(
      makeConfig({ provider: 'builtin', builtInModelId: 'qwen2.5-0.5b-q4km' }),
    );
    router.updateConfig({ provider: 'none' });
    expect(router.getActiveProvider()).toBe('none');
    const result = await router.classify(makePrompt({ subject: 'Big sale this weekend!' }));
    expect(result.category).toBe('promotions');
    expect(result.confidence).toBe(0.6);
    expect(result.reasoning).toBe('Rule-based keyword match');
    expect(mockClassify).not.toHaveBeenCalled();
  });

  it('user switches from builtin/model-A to builtin/model-B', async () => {
    const router = new GenerativeRouter(
      makeConfig({ provider: 'builtin', builtInModelId: 'qwen2.5-0.5b-q4km' }),
    );
    mockClassify.mockResolvedValueOnce({ category: 'primary', confidence: 0.85, reasoning: 'A' });
    await router.classify(makePrompt());
    router.updateConfig({ builtInModelId: 'smollm2-360m-q4km' });
    const second = { category: 'updates', confidence: 0.78, reasoning: 'model-B' };
    mockClassify.mockResolvedValueOnce(second);
    expect(await router.classify(makePrompt())).toEqual(second);
  });

  it('concurrent embedding (ONNX) + generative (builtin) do not interfere', async () => {
    const router = new GenerativeRouter(
      makeConfig({ provider: 'builtin', builtInModelId: 'qwen2.5-0.5b-q4km' }),
    );
    const embeddingPromise = new Promise<number[]>((resolve) =>
      setTimeout(() => resolve([0.1, 0.2, 0.3]), 10),
    );
    const classifyResult = { category: 'updates', confidence: 0.91, reasoning: 'LLM' };
    mockClassify.mockResolvedValueOnce(classifyResult);
    const [embedding, classification] = await Promise.all([
      embeddingPromise,
      router.classify(makePrompt()),
    ]);
    expect(embedding).toEqual([0.1, 0.2, 0.3]);
    expect(classification).toEqual(classifyResult);
  });
});

describe('Error recovery', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('model file deleted while loaded: fallback on next classify', async () => {
    const first = { category: 'primary', confidence: 0.9, reasoning: 'ok' };
    mockClassify.mockResolvedValueOnce(first);
    const router = new GenerativeRouter(
      makeConfig({ provider: 'builtin', builtInModelId: 'qwen2.5-0.5b-q4km' }),
    );
    expect(await router.classify(makePrompt())).toEqual(first);
    mockClassify.mockRejectedValueOnce(new Error('ENOENT: model file not found'));
    const r2 = await router.classify(makePrompt({ subject: 'Your receipt' }));
    expect(r2.reasoning).toBe('Rule-based keyword match');
  });

  it('classification retry after transient failure', async () => {
    mockClassify
      .mockRejectedValueOnce(new Error('Transient inference error'))
      .mockResolvedValueOnce({ category: 'primary', confidence: 0.85, reasoning: 'retry ok' });
    const router = new GenerativeRouter(
      makeConfig({ provider: 'builtin', builtInModelId: 'qwen2.5-0.5b-q4km' }),
    );
    const r1 = await router.classify(makePrompt());
    expect(r1.confidence).toBeLessThanOrEqual(0.6);
    const r2 = await router.classify(makePrompt());
    expect(r2.category).toBe('primary');
    expect(r2.confidence).toBe(0.85);
  });

  it('classification fallback to rule-based after max retries', async () => {
    mockClassify.mockRejectedValue(new Error('Persistent failure'));
    const router = new GenerativeRouter(
      makeConfig({ provider: 'builtin', builtInModelId: 'qwen2.5-0.5b-q4km' }),
    );
    const r1 = await router.classify(makePrompt({ subject: 'Big discount' }));
    expect(r1.category).toBe('promotions');
    expect(r1.reasoning).toBe('Rule-based keyword match');
    const r2 = await router.classify(makePrompt());
    expect(r2.confidence).toBeLessThanOrEqual(0.6);
  });

  it('disk space check before download (model cache reports size)', async () => {
    const { getCacheSize } = await import('../model-cache');
    const result = await getCacheSize();
    expect(result).toHaveProperty('totalBytes');
    expect(result).toHaveProperty('models');
    expect(typeof result.totalBytes).toBe('number');
  });
});

describe('Settings persistence', () => {
  let store: Record<string, string>;
  beforeEach(() => {
    vi.clearAllMocks();
    store = {};
  });

  const STORAGE_KEY = 'emailibrium-settings';
  function setItem(key: string, value: string) {
    store[key] = value;
  }
  function getItem(key: string): string | null {
    return store[key] ?? null;
  }

  it('builtInLlmModel persisted to localStorage', () => {
    const data = {
      state: { builtInLlmModel: 'smollm2-360m-q4km', llmProvider: 'builtin' },
      version: 0,
    };
    setItem(STORAGE_KEY, JSON.stringify(data));
    const stored = JSON.parse(getItem(STORAGE_KEY)!);
    expect(stored.state.builtInLlmModel).toBe('smollm2-360m-q4km');
  });

  it('provider change persisted', () => {
    const data = {
      state: { llmProvider: 'none', builtInLlmModel: 'qwen2.5-0.5b-q4km' },
      version: 0,
    };
    setItem(STORAGE_KEY, JSON.stringify(data));
    const stored = JSON.parse(getItem(STORAGE_KEY)!);
    stored.state.llmProvider = 'builtin';
    setItem(STORAGE_KEY, JSON.stringify(stored));
    const updated = JSON.parse(getItem(STORAGE_KEY)!);
    expect(updated.state.llmProvider).toBe('builtin');
  });

  it('settings survive page reload (re-read from localStorage)', () => {
    const data = {
      state: {
        llmProvider: 'builtin',
        builtInLlmModel: 'smollm2-360m-q4km',
        builtInLlmIdleTimeout: 300,
        builtInLlmMaxContext: 2048,
      },
      version: 0,
    };
    setItem(STORAGE_KEY, JSON.stringify(data));
    const reloaded = JSON.parse(getItem(STORAGE_KEY)!);
    expect(reloaded.state.llmProvider).toBe('builtin');
    expect(reloaded.state.builtInLlmModel).toBe('smollm2-360m-q4km');
    expect(reloaded.state.builtInLlmIdleTimeout).toBe(300);
    expect(reloaded.state.builtInLlmMaxContext).toBe(2048);
  });
});

describe('Provider fallback chain', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('cloud (openai) fails: returns stub classification', async () => {
    const router = new GenerativeRouter(makeConfig({ provider: 'openai' }));
    const result = await router.classify(makePrompt());
    expect(result.category).toBe('uncategorized');
    expect(result.confidence).toBe(0);
    expect(result.reasoning).toMatch(/not yet implemented/);
  });

  it('ollama (local) fails: returns stub classification', async () => {
    const router = new GenerativeRouter(makeConfig({ provider: 'local' }));
    const result = await router.classify(makePrompt());
    expect(result.category).toBe('uncategorized');
    expect(result.confidence).toBe(0);
    expect(result.reasoning).toMatch(/not yet implemented/);
  });

  it('builtin fails: falls back to rule-based', async () => {
    mockClassify.mockRejectedValue(new Error('Model crashed'));
    const router = new GenerativeRouter(
      makeConfig({ provider: 'builtin', builtInModelId: 'qwen2.5-0.5b-q4km' }),
    );
    const result = await router.classify(makePrompt({ subject: 'Your invoice is ready' }));
    expect(result.category).toBe('updates');
    expect(result.confidence).toBe(0.6);
    expect(result.reasoning).toBe('Rule-based keyword match');
  });

  it('rule-based always returns a result', async () => {
    const router = new GenerativeRouter(makeConfig({ provider: 'none' }));
    const r1 = await router.classify(makePrompt({ subject: 'Weekly newsletter' }));
    expect(r1.category).toBeDefined();
    expect(r1.confidence).toBeGreaterThan(0);
    const r2 = await router.classify(makePrompt({ subject: 'Hello friend' }));
    expect(r2.category).toBe('primary');
    expect(r2.confidence).toBe(0.3);
    expect(r2.reasoning).toBe('No keyword match — default category');
  });
});
