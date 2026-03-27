import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// ---------------------------------------------------------------------------
// Mocks (vitest 4: hoist mock variables)
// ---------------------------------------------------------------------------

const { mockClassify } = vi.hoisted(() => ({
  mockClassify: vi.fn(),
}));

import { classifyWithRecovery, ClassificationQueue, checkDiskSpace } from '../error-recovery';
import type { ClassificationPrompt, ClassificationResult } from '../built-in-llm-adapter';
import type { BuiltInLlmManager } from '../built-in-llm-manager';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeManager(): BuiltInLlmManager {
  return { classify: mockClassify } as unknown as BuiltInLlmManager;
}

function makePrompt(overrides: Partial<ClassificationPrompt> = {}): ClassificationPrompt {
  return {
    subject: 'Hello',
    sender: 'alice@example.com',
    bodyPreview: 'Just checking in.',
    categories: ['primary', 'updates', 'promotions'],
    ...overrides,
  };
}

const successResult: ClassificationResult = {
  category: 'primary',
  confidence: 0.95,
  reasoning: 'Test classification',
};

// ---------------------------------------------------------------------------
// classifyWithRecovery
// ---------------------------------------------------------------------------

describe('classifyWithRecovery', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockClassify.mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns classify result on first success', async () => {
    mockClassify.mockResolvedValueOnce(successResult);

    const promise = classifyWithRecovery(makeManager(), makePrompt());
    const result = await promise;

    expect(result).toEqual({ ...successResult, fallback: false });
    expect(mockClassify).toHaveBeenCalledTimes(1);
  });

  it('retries on failure and succeeds on second attempt', async () => {
    mockClassify.mockRejectedValueOnce(new Error('transient')).mockResolvedValueOnce(successResult);

    const promise = classifyWithRecovery(makeManager(), makePrompt(), {
      maxRetries: 2,
      retryDelayMs: 1000,
    });

    // Advance past the first retry delay (1000ms * 2^0 = 1000ms)
    await vi.advanceTimersByTimeAsync(1000);

    const result = await promise;
    expect(result).toEqual({ ...successResult, fallback: false });
    expect(mockClassify).toHaveBeenCalledTimes(2);
  });

  it('calls onError callback on each failure', async () => {
    const onError = vi.fn();
    mockClassify
      .mockRejectedValueOnce(new Error('fail1'))
      .mockRejectedValueOnce(new Error('fail2'))
      .mockRejectedValueOnce(new Error('fail3'));

    const promise = classifyWithRecovery(makeManager(), makePrompt(), {
      maxRetries: 2,
      retryDelayMs: 100,
      onError,
    });

    // Advance through retry delays: 100ms * 2^0 = 100, 100ms * 2^1 = 200
    await vi.advanceTimersByTimeAsync(100);
    await vi.advanceTimersByTimeAsync(200);

    const result = await promise;

    // All retries exhausted => fallback
    expect(result.fallback).toBe(true);
    expect(onError).toHaveBeenCalledTimes(3);
    expect(onError).toHaveBeenCalledWith(expect.any(Error), 0);
    expect(onError).toHaveBeenCalledWith(expect.any(Error), 1);
    expect(onError).toHaveBeenCalledWith(expect.any(Error), 2);
  });

  it('returns rule-based fallback when all retries exhausted', async () => {
    mockClassify.mockRejectedValue(new Error('always fails'));

    const prompt = makePrompt({ subject: 'Your invoice is ready' });
    const promise = classifyWithRecovery(makeManager(), prompt, {
      maxRetries: 1,
      retryDelayMs: 50,
    });

    await vi.advanceTimersByTimeAsync(50);

    const result = await promise;
    expect(result.fallback).toBe(true);
    // "invoice" keyword should match "updates"
    expect(result.category).toBe('updates');
    expect(result.confidence).toBe(0.6);
  });

  it('falls back to default category when no keyword matches', async () => {
    mockClassify.mockRejectedValue(new Error('always fails'));

    const prompt = makePrompt({
      subject: 'Random subject',
      sender: 'bob@test.com',
      bodyPreview: 'Nothing special here',
    });
    const promise = classifyWithRecovery(makeManager(), prompt, {
      maxRetries: 0,
      retryDelayMs: 10,
    });

    const result = await promise;
    expect(result.fallback).toBe(true);
    expect(result.category).toBe('primary'); // first category in list
    expect(result.confidence).toBe(0.3);
  });

  it('matches promotional keywords in fallback', async () => {
    mockClassify.mockRejectedValue(new Error('fail'));

    const prompt = makePrompt({ subject: 'Big sale this weekend!' });
    const promise = classifyWithRecovery(makeManager(), prompt, {
      maxRetries: 0,
    });

    const result = await promise;
    expect(result.fallback).toBe(true);
    expect(result.category).toBe('promotions');
  });

  it('uses exponential backoff delays', async () => {
    mockClassify
      .mockRejectedValueOnce(new Error('fail1'))
      .mockRejectedValueOnce(new Error('fail2'))
      .mockResolvedValueOnce(successResult);

    const promise = classifyWithRecovery(makeManager(), makePrompt(), {
      maxRetries: 2,
      retryDelayMs: 1000,
    });

    // First retry: 1000 * 2^0 = 1000ms
    await vi.advanceTimersByTimeAsync(1000);
    expect(mockClassify).toHaveBeenCalledTimes(2);

    // Second retry: 1000 * 2^1 = 2000ms
    await vi.advanceTimersByTimeAsync(2000);

    const result = await promise;
    expect(result.fallback).toBe(false);
    expect(mockClassify).toHaveBeenCalledTimes(3);
  });
});

// ---------------------------------------------------------------------------
// ClassificationQueue
// ---------------------------------------------------------------------------

describe('ClassificationQueue', () => {
  beforeEach(() => {
    mockClassify.mockReset();
  });

  it('serialises concurrent classify calls', async () => {
    const callOrder: number[] = [];

    mockClassify.mockImplementation(async () => {
      const idx = callOrder.length;
      callOrder.push(idx);
      // Simulate async work
      await new Promise((r) => setTimeout(r, 10));
      return { ...successResult, reasoning: `call-${idx}` };
    });

    const queue = new ClassificationQueue(makeManager());
    const p1 = queue.enqueue(makePrompt({ subject: 'A' }));
    const p2 = queue.enqueue(makePrompt({ subject: 'B' }));
    const p3 = queue.enqueue(makePrompt({ subject: 'C' }));

    expect(queue.pending).toBe(3);

    const results = await Promise.all([p1, p2, p3]);

    expect(results[0].reasoning).toBe('call-0');
    expect(results[1].reasoning).toBe('call-1');
    expect(results[2].reasoning).toBe('call-2');
    expect(callOrder).toEqual([0, 1, 2]);
  });

  it('tracks pending count correctly', async () => {
    mockClassify.mockResolvedValue(successResult);

    const queue = new ClassificationQueue(makeManager());
    expect(queue.pending).toBe(0);

    const p = queue.enqueue(makePrompt());
    expect(queue.pending).toBe(1);

    await p;
    // After resolution, pending should decrement
    // We need a microtask for the finally to run
    await new Promise((r) => setTimeout(r, 0));
    expect(queue.pending).toBe(0);
  });

  it('rejects enqueue after cancel', async () => {
    const queue = new ClassificationQueue(makeManager());
    queue.cancel();

    await expect(queue.enqueue(makePrompt())).rejects.toThrow('Queue has been cancelled');
  });

  it('cancels pending work mid-flight', async () => {
    let resolveFirst: (() => void) | undefined;
    mockClassify.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveFirst = () => resolve(successResult);
        }),
    );
    mockClassify.mockResolvedValue(successResult);

    const queue = new ClassificationQueue(makeManager());
    const p1 = queue.enqueue(makePrompt());

    // Let the microtask for p1's .then() run so classify gets called
    await Promise.resolve();

    const p2 = queue.enqueue(makePrompt());

    // Cancel while first is still in progress
    queue.cancel();

    // Resolve the first call
    resolveFirst?.();
    const r1 = await p1;
    expect(r1).toEqual(successResult);

    // Second should be rejected because queue was cancelled
    await expect(p2).rejects.toThrow('Queue has been cancelled');
  });

  it('continues processing after an individual failure', async () => {
    mockClassify
      .mockRejectedValueOnce(new Error('first fails'))
      .mockResolvedValueOnce({ ...successResult, reasoning: 'second succeeds' });

    const queue = new ClassificationQueue(makeManager());
    const p1 = queue.enqueue(makePrompt());
    const p2 = queue.enqueue(makePrompt());

    await expect(p1).rejects.toThrow('first fails');
    const r2 = await p2;
    expect(r2.reasoning).toBe('second succeeds');
  });
});

// ---------------------------------------------------------------------------
// checkDiskSpace
// ---------------------------------------------------------------------------

describe('checkDiskSpace', () => {
  it('returns hasSpace: true in browser context (no fs module)', async () => {
    // In a vitest/browser-like context, dynamic import of fs will fail
    const result = await checkDiskSpace(500_000_000);
    expect(result.hasSpace).toBe(true);
  });
});
