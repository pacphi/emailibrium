/**
 * Configuration for retry behavior with exponential backoff.
 */
export interface RetryOptions {
  /** Maximum number of retry attempts. Defaults to 3. */
  maxRetries?: number;
  /** Base delay in milliseconds before the first retry. Defaults to 1000. */
  baseDelay?: number;
  /** Maximum delay cap in milliseconds. Defaults to 30000. */
  maxDelay?: number;
  /** Optional predicate to decide if a given error is retryable. Defaults to always true. */
  shouldRetry?: (error: unknown, attempt: number) => boolean;
}

const DEFAULT_MAX_RETRIES = 3;
const DEFAULT_BASE_DELAY = 1000;
const DEFAULT_MAX_DELAY = 30000;

/**
 * Executes an async function with exponential backoff retry logic.
 *
 * The delay between retries follows: min(baseDelay * 2^attempt, maxDelay)
 * with a small random jitter to avoid thundering herd problems.
 *
 * @param fn - The async function to execute.
 * @param options - Retry configuration.
 * @returns The resolved value from the function.
 * @throws The last error encountered after all retries are exhausted.
 */
export async function withRetry<T>(fn: () => Promise<T>, options?: RetryOptions): Promise<T> {
  const maxRetries = options?.maxRetries ?? DEFAULT_MAX_RETRIES;
  const baseDelay = options?.baseDelay ?? DEFAULT_BASE_DELAY;
  const maxDelay = options?.maxDelay ?? DEFAULT_MAX_DELAY;
  const shouldRetry = options?.shouldRetry ?? (() => true);

  let lastError: unknown;

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      return await fn();
    } catch (error: unknown) {
      lastError = error;

      const isLastAttempt = attempt === maxRetries;
      if (isLastAttempt || !shouldRetry(error, attempt)) {
        throw error;
      }

      const exponentialDelay = baseDelay * Math.pow(2, attempt);
      const clampedDelay = Math.min(exponentialDelay, maxDelay);
      // Add jitter: 0-25% of the computed delay
      const jitter = clampedDelay * Math.random() * 0.25;
      const delay = clampedDelay + jitter;

      await sleep(delay);
    }
  }

  // Unreachable, but satisfies the type checker
  throw lastError;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
