import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { withRetry } from '../retry';

describe('withRetry', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  // -----------------------------------------------------------------------
  // Success cases
  // -----------------------------------------------------------------------

  it('returns immediately on first success', async () => {
    const fn = vi.fn().mockResolvedValue('ok');

    const promise = withRetry(fn);
    const result = await promise;

    expect(result).toBe('ok');
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('succeeds after transient failures', async () => {
    const fn = vi.fn()
      .mockRejectedValueOnce(new Error('fail1'))
      .mockRejectedValueOnce(new Error('fail2'))
      .mockResolvedValueOnce('recovered');

    const promise = withRetry(fn, { maxRetries: 3, baseDelay: 100, maxDelay: 10000 });

    // First retry delay: ~100ms (100 * 2^0 + jitter)
    await vi.advanceTimersByTimeAsync(150);
    // Second retry delay: ~200ms (100 * 2^1 + jitter)
    await vi.advanceTimersByTimeAsync(300);

    const result = await promise;
    expect(result).toBe('recovered');
    expect(fn).toHaveBeenCalledTimes(3);
  });

  // -----------------------------------------------------------------------
  // Exhaustion
  // -----------------------------------------------------------------------

  it('throws the last error after all retries exhausted', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0);
    const fn = vi.fn().mockImplementation(async () => { throw new Error('persistent'); });

    const promise = withRetry(fn, { maxRetries: 2, baseDelay: 50, maxDelay: 10000 });

    // Attach catch handler immediately to avoid unhandled rejection
    const caughtPromise = promise.catch((e: Error) => e);

    // Retry 0: 50 * 2^0 = 50ms
    await vi.advanceTimersByTimeAsync(50);
    // Retry 1: 50 * 2^1 = 100ms
    await vi.advanceTimersByTimeAsync(100);

    const error = await caughtPromise;
    expect(error).toBeInstanceOf(Error);
    expect((error as Error).message).toBe('persistent');
    expect(fn).toHaveBeenCalledTimes(3); // initial + 2 retries
    vi.restoreAllMocks();
  });

  it('throws immediately with maxRetries=0', async () => {
    const fn = vi.fn().mockImplementation(() => Promise.reject(new Error('no retries')));

    await expect(withRetry(fn, { maxRetries: 0 })).rejects.toThrow('no retries');
    expect(fn).toHaveBeenCalledTimes(1);
  });

  // -----------------------------------------------------------------------
  // shouldRetry predicate
  // -----------------------------------------------------------------------

  it('stops retrying when shouldRetry returns false', async () => {
    const fn = vi.fn().mockImplementation(() => Promise.reject(new Error('non-retryable')));
    const shouldRetry = vi.fn().mockReturnValue(false);

    await expect(
      withRetry(fn, { maxRetries: 5, shouldRetry }),
    ).rejects.toThrow('non-retryable');

    expect(fn).toHaveBeenCalledTimes(1);
    expect(shouldRetry).toHaveBeenCalledWith(expect.any(Error), 0);
  });

  it('respects shouldRetry allowing some retries then stopping', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0);
    const fn = vi.fn().mockImplementation(async () => { throw new Error('fail'); });
    // Allow first retry (attempt 0), reject second (attempt 1)
    const shouldRetry = vi.fn()
      .mockReturnValueOnce(true)
      .mockReturnValueOnce(false);

    const promise = withRetry(fn, { maxRetries: 5, baseDelay: 50, maxDelay: 10000, shouldRetry });

    // Attach catch handler immediately to avoid unhandled rejection
    const caughtPromise = promise.catch((e: Error) => e);

    // Retry 0: 50 * 2^0 = 50ms
    await vi.advanceTimersByTimeAsync(50);

    const error = await caughtPromise;
    expect(error).toBeInstanceOf(Error);
    expect((error as Error).message).toBe('fail');
    expect(fn).toHaveBeenCalledTimes(2); // initial + 1 retry
    vi.restoreAllMocks();
  });

  // -----------------------------------------------------------------------
  // Exponential backoff timing
  // -----------------------------------------------------------------------

  it('uses exponential backoff with delays doubling', async () => {
    const fn = vi.fn()
      .mockRejectedValueOnce(new Error('1'))
      .mockRejectedValueOnce(new Error('2'))
      .mockRejectedValueOnce(new Error('3'))
      .mockResolvedValueOnce('done');

    // Fix Math.random to 0 so jitter is 0
    vi.spyOn(Math, 'random').mockReturnValue(0);

    const promise = withRetry(fn, { maxRetries: 3, baseDelay: 1000, maxDelay: 30000 });

    // Retry 0: 1000 * 2^0 = 1000ms
    expect(fn).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1000);
    expect(fn).toHaveBeenCalledTimes(2);

    // Retry 1: 1000 * 2^1 = 2000ms
    await vi.advanceTimersByTimeAsync(2000);
    expect(fn).toHaveBeenCalledTimes(3);

    // Retry 2: 1000 * 2^2 = 4000ms
    await vi.advanceTimersByTimeAsync(4000);
    expect(fn).toHaveBeenCalledTimes(4);

    const result = await promise;
    expect(result).toBe('done');

    vi.restoreAllMocks();
  });

  // -----------------------------------------------------------------------
  // maxDelay capping
  // -----------------------------------------------------------------------

  it('clamps delay to maxDelay', async () => {
    const fn = vi.fn()
      .mockRejectedValueOnce(new Error('1'))
      .mockResolvedValueOnce('done');

    vi.spyOn(Math, 'random').mockReturnValue(0);

    // baseDelay=5000, attempt=0 => 5000*2^0 = 5000, but maxDelay=2000
    const promise = withRetry(fn, { maxRetries: 1, baseDelay: 5000, maxDelay: 2000 });

    await vi.advanceTimersByTimeAsync(2000);

    const result = await promise;
    expect(result).toBe('done');
    expect(fn).toHaveBeenCalledTimes(2);

    vi.restoreAllMocks();
  });

  // -----------------------------------------------------------------------
  // Jitter bounds
  // -----------------------------------------------------------------------

  it('adds jitter of 0-25% to the delay', async () => {
    const fn = vi.fn()
      .mockRejectedValueOnce(new Error('fail'))
      .mockResolvedValueOnce('ok');

    // Math.random returns 1.0 => jitter = clampedDelay * 1.0 * 0.25 = 25%
    vi.spyOn(Math, 'random').mockReturnValue(1.0);

    const promise = withRetry(fn, { maxRetries: 1, baseDelay: 1000, maxDelay: 30000 });

    // Delay = 1000 + 1000*1.0*0.25 = 1250ms
    // At 1000ms it shouldn't have retried yet
    await vi.advanceTimersByTimeAsync(1000);
    expect(fn).toHaveBeenCalledTimes(1);

    // At 1250ms it should retry
    await vi.advanceTimersByTimeAsync(250);
    expect(fn).toHaveBeenCalledTimes(2);

    const result = await promise;
    expect(result).toBe('ok');

    vi.restoreAllMocks();
  });

  it('has zero jitter when Math.random returns 0', async () => {
    const fn = vi.fn()
      .mockRejectedValueOnce(new Error('fail'))
      .mockResolvedValueOnce('ok');

    vi.spyOn(Math, 'random').mockReturnValue(0);

    const promise = withRetry(fn, { maxRetries: 1, baseDelay: 1000, maxDelay: 30000 });

    // Delay = 1000 + 0 = 1000ms exactly
    await vi.advanceTimersByTimeAsync(1000);
    expect(fn).toHaveBeenCalledTimes(2);

    const result = await promise;
    expect(result).toBe('ok');

    vi.restoreAllMocks();
  });
});
