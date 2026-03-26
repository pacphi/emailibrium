/**
 * BL-4.04 -- Error recovery and resilience utilities.
 *
 * Wraps BuiltInLlmManager with retry logic, a serialised classification
 * queue, and a disk-space pre-flight check.
 */

import type { BuiltInLlmManager } from './built-in-llm-manager';
import type { ClassificationPrompt, ClassificationResult } from './built-in-llm-adapter';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface RecoveryOptions {
  maxRetries?: number;
  retryDelayMs?: number;
  onError?: (error: Error, attempt: number) => void;
}

export interface DiskSpaceResult {
  hasSpace: boolean;
  availableBytes?: number;
  message?: string;
}

// ---------------------------------------------------------------------------
// Rule-based fallback (mirrors generative-router's keyword logic)
// ---------------------------------------------------------------------------

function ruleBasedFallback(
  prompt: ClassificationPrompt,
): ClassificationResult & { fallback: boolean } {
  const text = `${prompt.subject} ${prompt.sender} ${prompt.bodyPreview}`.toLowerCase();

  const rules: [string[], string][] = [
    [['invoice', 'receipt', 'payment', 'billing', 'subscription'], 'updates'],
    [['sale', 'discount', 'offer', 'promo', 'deal', 'coupon', 'unsubscribe'], 'promotions'],
    [['meeting', 'calendar', 'schedule', 'invite', 'rsvp'], 'updates'],
    [['github.com', 'gitlab', 'jira', 'slack', 'notifications'], 'updates'],
    [['newsletter', 'digest', 'weekly', 'roundup'], 'promotions'],
  ];

  for (const [keywords, category] of rules) {
    if (keywords.some((kw) => text.includes(kw))) {
      return { category, confidence: 0.6, reasoning: 'Rule-based fallback', fallback: true };
    }
  }

  return {
    category: prompt.categories[0] ?? 'primary',
    confidence: 0.3,
    reasoning: 'No keyword match — default category',
    fallback: true,
  };
}

// ---------------------------------------------------------------------------
// classifyWithRecovery
// ---------------------------------------------------------------------------

const DEFAULT_MAX_RETRIES = 2;
const DEFAULT_RETRY_DELAY_MS = 1000;

/**
 * Wraps a classify call with retry and exponential-backoff logic.
 * On failure after all retries, returns a rule-based fallback result.
 */
export async function classifyWithRecovery(
  manager: BuiltInLlmManager,
  prompt: ClassificationPrompt,
  options?: RecoveryOptions,
): Promise<ClassificationResult & { fallback: boolean }> {
  const maxRetries = options?.maxRetries ?? DEFAULT_MAX_RETRIES;
  const baseDelay = options?.retryDelayMs ?? DEFAULT_RETRY_DELAY_MS;

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      const result = await manager.classify(prompt);
      return { ...result, fallback: false };
    } catch (err) {
      const error = err instanceof Error ? err : new Error(String(err));
      options?.onError?.(error, attempt);

      if (attempt < maxRetries) {
        const delay = baseDelay * 2 ** attempt; // 1s, 2s
        await new Promise((r) => setTimeout(r, delay));
      }
    }
  }

  return ruleBasedFallback(prompt);
}

// ---------------------------------------------------------------------------
// ClassificationQueue
// ---------------------------------------------------------------------------

/**
 * Serialises concurrent classification requests into a FIFO queue.
 * Prevents multiple simultaneous model loads or inference calls.
 */
export class ClassificationQueue {
  private manager: BuiltInLlmManager;
  private chain: Promise<unknown> = Promise.resolve();
  private pendingCount = 0;
  private cancelled = false;

  constructor(manager: BuiltInLlmManager) {
    this.manager = manager;
  }

  /** Queue a classification request. */
  enqueue(prompt: ClassificationPrompt): Promise<ClassificationResult> {
    if (this.cancelled) {
      return Promise.reject(new Error('Queue has been cancelled'));
    }

    this.pendingCount++;

    const result = this.chain.then(async () => {
      if (this.cancelled) throw new Error('Queue has been cancelled');
      return this.manager.classify(prompt);
    });

    // Advance the chain regardless of success/failure
    this.chain = result
      .catch(() => {})
      .finally(() => {
        this.pendingCount--;
      });

    return result;
  }

  /** Number of pending requests. */
  get pending(): number {
    return this.pendingCount;
  }

  /** Cancel all pending requests. */
  cancel(): void {
    this.cancelled = true;
    this.pendingCount = 0;
    this.chain = Promise.resolve();
  }
}

// ---------------------------------------------------------------------------
// checkDiskSpace
// ---------------------------------------------------------------------------

/**
 * Check if the disk has enough space for a model download.
 * Uses Node.js `fs.statfs` when available; otherwise assumes space is
 * available (browser context).
 */
export async function checkDiskSpace(requiredBytes: number): Promise<DiskSpaceResult> {
  try {
    // Dynamic require to avoid bundler / TS errors in browser contexts

    const fs = await (Function('return import("fs/promises")')() as Promise<{
      statfs?: (path: string) => Promise<{ bfree: number; bsize: number }>;
    }>);
    if (typeof fs.statfs === 'function') {
      const stats = await fs.statfs('/');
      const availableBytes = stats.bfree * stats.bsize;
      const hasSpace = availableBytes >= requiredBytes;
      return {
        hasSpace,
        availableBytes,
        message: hasSpace
          ? undefined
          : `Need ${(requiredBytes / 1e6).toFixed(0)} MB but only ${(availableBytes / 1e6).toFixed(0)} MB available`,
      };
    }
  } catch {
    // fs module unavailable (browser context) — assume space is available
  }

  return { hasSpace: true };
}
