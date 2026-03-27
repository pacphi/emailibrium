/**
 * BL-2.04 — BuiltInLlmManager orchestrator.
 *
 * Single entry point for the rest of the app to interact with the built-in LLM.
 * Coordinates HardwareDetector, ModelDownloader, ModelCache, and BuiltInLlmAdapter.
 */

import type { HardwareInfo } from './hardware-detector';
import type { ModelManifest } from './model-manifest';
import type {
  ClassificationPrompt,
  ClassificationResult,
  Message,
  Completion,
  CompletionOptions,
  StreamOptions,
  Token,
  BuiltInLlmAdapter,
} from './built-in-llm-adapter';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface BuiltInLlmManagerConfig {
  /** Model identifier. Defaults to DEFAULT_MODEL_ID from manifest. */
  modelId?: string;
  /** Auto-unload after this many ms of inactivity. @default 300_000 */
  idleTimeoutMs?: number;
  /** Context window size in tokens. @default 2048 */
  contextSize?: number;
}

export interface MemoryCheck {
  canLoad: boolean;
  availableMb: number;
  requiredMb: number;
  warning?: string;
}

export interface ManagerStatus {
  modelId: string;
  isModelCached: boolean;
  isModelLoaded: boolean;
  hardware: HardwareInfo | null;
  /** 0-100 if a download is in progress, null otherwise. */
  downloadProgress: number | null;
  memoryWarning?: string;
  memoryUsageMb?: number;
}

export interface ProgressInfo {
  percent: number;
  bytesDownloaded: number;
  totalBytes: number;
}

type ProgressCallback = (progress: ProgressInfo) => void;

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

const DEFAULT_IDLE_TIMEOUT_MS = 300_000;
const DEFAULT_CONTEXT_SIZE = 2048;
const LOW_RAM_IDLE_TIMEOUT_MS = 120_000;
const MEMORY_SAFETY_MARGIN = 1.2;
const MEMORY_MONITOR_INTERVAL_MS = 30_000;
const MEMORY_WARNING_THRESHOLD = 0.8;
const LOW_RAM_THRESHOLD_GB = 8;

/**
 * Top-level orchestrator that coordinates hardware detection, model
 * downloading/caching, and the LLM adapter lifecycle.
 */
export class BuiltInLlmManager {
  private modelId: string;
  private readonly idleTimeoutMs: number;
  private readonly contextSize: number;

  private adapter: BuiltInLlmAdapter | null = null;
  private hardware: HardwareInfo | null = null;
  private manifest: ModelManifest | null = null;
  private initPromise: Promise<void> | null = null;
  private currentProgress: number | null = null;
  private progressListeners = new Set<ProgressCallback>();
  private memoryMonitorTimer: ReturnType<typeof setInterval> | null = null;
  private memoryWarning: string | undefined;

  constructor(config?: BuiltInLlmManagerConfig) {
    this.modelId = config?.modelId ?? '';
    this.idleTimeoutMs = config?.idleTimeoutMs ?? DEFAULT_IDLE_TIMEOUT_MS;
    this.contextSize = config?.contextSize ?? DEFAULT_CONTEXT_SIZE;
  }

  // -----------------------------------------------------------------------
  // Lifecycle
  // -----------------------------------------------------------------------

  /** Download model if needed, verify, and load. */
  async initialize(): Promise<void> {
    // Resolve the default model ID lazily so we only import manifest once needed
    if (!this.modelId) {
      const { DEFAULT_MODEL_ID } = await import('./model-manifest');
      this.modelId = DEFAULT_MODEL_ID;
    }

    // 1. Get manifest
    const { getManifest } = await import('./model-manifest');
    const manifest = getManifest(this.modelId);
    if (!manifest) {
      throw new Error(`Unknown model: ${this.modelId}`);
    }
    this.manifest = manifest;

    // 2. Detect hardware
    const { detectHardware } = await import('./hardware-detector');
    this.hardware = await detectHardware();

    // 3. Check cache
    const { isModelCached, getModelPath, ensureCacheDir } = await import('./model-cache');
    const cached = await isModelCached(manifest);

    // 4. Download if needed
    let modelPath: string | null;

    if (!cached) {
      const cacheDir = await ensureCacheDir();
      const { downloadModel } = await import('./model-downloader');

      this.currentProgress = 0;

      modelPath = await downloadModel(manifest, cacheDir, {
        onProgress: ({ percent, bytesDownloaded, totalBytes }) => {
          this.currentProgress = percent;
          this.broadcastProgress({ percent, bytesDownloaded, totalBytes });
        },
      });

      this.currentProgress = null;
    } else {
      // 5. Get cached model path
      modelPath = await getModelPath(manifest);
    }

    if (!modelPath) {
      throw new Error(`Failed to resolve model path for ${this.modelId}`);
    }

    // 6. Check memory availability
    const memCheck = this.checkMemoryAvailability(manifest);
    if (!memCheck.canLoad) {
      const { InsufficientMemoryError } = await import('./built-in-llm-adapter');
      throw new InsufficientMemoryError(memCheck.warning);
    }
    if (memCheck.warning) {
      this.memoryWarning = memCheck.warning;
    }

    // 7. Create adapter and load (with adaptive idle timeout)
    const { BuiltInLlmAdapter: AdapterClass } = await import('./built-in-llm-adapter');

    this.adapter = new AdapterClass({
      modelPath,
      contextSize: this.contextSize,
      idleTimeoutMs: this.getEffectiveIdleTimeout(),
    });

    // 8. Load model
    await this.adapter.load();

    // 9. Start memory monitor
    this.startMemoryMonitor();
  }

  // -----------------------------------------------------------------------
  // Memory management
  // -----------------------------------------------------------------------

  /** Estimate available system memory and check against model requirements. */
  checkMemoryAvailability(manifest: ModelManifest): MemoryCheck {
    const requiredBytes = manifest.ramEstimateBytes * MEMORY_SAFETY_MARGIN;
    const requiredMb = Math.ceil(requiredBytes / (1024 * 1024));

    let availableMb: number;

    if (typeof process !== 'undefined' && process.memoryUsage) {
      // Node.js / Electron environment — estimate from heap limit and RSS.
      const mem = process.memoryUsage();
      // Use total system memory when available via os module, otherwise
      // fall back to a rough heuristic based on RSS headroom.
      try {
        // eslint-disable-next-line @typescript-eslint/no-require-imports
        const os = require('os');
        const freeMem: number = os.freemem();
        availableMb = Math.floor(freeMem / (1024 * 1024));
      } catch {
        // Fallback: assume 4 GB total minus current RSS.
        const assumedTotal = 4 * 1024 * 1024 * 1024;
        availableMb = Math.floor((assumedTotal - mem.rss) / (1024 * 1024));
      }
    } else if (
      typeof navigator !== 'undefined' &&
      (navigator as { deviceMemory?: number }).deviceMemory
    ) {
      // Browser environment — navigator.deviceMemory gives approximate GB.
      const deviceGb = (navigator as { deviceMemory?: number }).deviceMemory!;
      // Assume ~50% of device memory is available.
      availableMb = Math.floor((deviceGb * 1024) / 2);
    } else {
      // Unknown environment — assume 2 GB available as conservative default.
      availableMb = 2048;
    }

    if (availableMb < requiredMb) {
      return {
        canLoad: false,
        availableMb,
        requiredMb,
        warning: `Insufficient memory: ${availableMb} MB available, ${requiredMb} MB required (including 20% safety margin).`,
      };
    }

    // Tight but loadable: available < 1.5x required.
    const tightThreshold = requiredMb * 1.25;
    if (availableMb < tightThreshold) {
      return {
        canLoad: true,
        availableMb,
        requiredMb,
        warning: `Memory is tight: ${availableMb} MB available for ${requiredMb} MB required. Performance may be degraded.`,
      };
    }

    return { canLoad: true, availableMb, requiredMb };
  }

  /** Start periodic memory monitoring after model load. */
  private startMemoryMonitor(): void {
    this.stopMemoryMonitor();

    if (typeof process === 'undefined' || !process.memoryUsage) return;

    this.memoryMonitorTimer = setInterval(() => {
      try {
        const mem = process.memoryUsage();
        // eslint-disable-next-line @typescript-eslint/no-require-imports
        const os = require('os');
        const totalMem: number = os.totalmem();
        const ratio = mem.rss / totalMem;

        if (ratio > MEMORY_WARNING_THRESHOLD) {
          const rssMb = Math.round(mem.rss / (1024 * 1024));
          const totalMb = Math.round(totalMem / (1024 * 1024));
          console.warn(
            `[BuiltInLlmManager] High memory usage: RSS ${rssMb} MB / ${totalMb} MB (${Math.round(ratio * 100)}%)`,
          );
        }
      } catch {
        // Cannot read memory info — silently ignore.
      }
    }, MEMORY_MONITOR_INTERVAL_MS);
  }

  /** Stop the periodic memory monitor. */
  private stopMemoryMonitor(): void {
    if (this.memoryMonitorTimer !== null) {
      clearInterval(this.memoryMonitorTimer);
      this.memoryMonitorTimer = null;
    }
  }

  /** Get effective idle timeout, reduced for memory-constrained systems. */
  private getEffectiveIdleTimeout(): number {
    let systemRamGb = Infinity;
    try {
      if (typeof process !== 'undefined') {
        // eslint-disable-next-line @typescript-eslint/no-require-imports
        const os = require('os');
        systemRamGb = os.totalmem() / (1024 * 1024 * 1024);
      } else if (
        typeof navigator !== 'undefined' &&
        (navigator as { deviceMemory?: number }).deviceMemory
      ) {
        systemRamGb = (navigator as { deviceMemory?: number }).deviceMemory!;
      }
    } catch {
      // Unknown — use configured timeout.
    }

    if (systemRamGb < LOW_RAM_THRESHOLD_GB) {
      return Math.min(this.idleTimeoutMs, LOW_RAM_IDLE_TIMEOUT_MS);
    }
    return this.idleTimeoutMs;
  }

  // -----------------------------------------------------------------------
  // Inference — lazy-initialized
  // -----------------------------------------------------------------------

  /** Classify an email. Lazy-initializes if needed. */
  async classify(prompt: ClassificationPrompt): Promise<ClassificationResult> {
    await this.ensureInitialized();
    return this.adapter!.classify(prompt);
  }

  /** Chat completion. Lazy-initializes if needed. */
  async chat(messages: Message[], options?: CompletionOptions): Promise<Completion> {
    await this.ensureInitialized();
    return this.adapter!.complete(messages, options);
  }

  /** Streaming chat. Lazy-initializes if needed. */
  async *chatStream(
    messages: Message[],
    options?: StreamOptions,
  ): AsyncGenerator<Token, Completion, void> {
    await this.ensureInitialized();
    return yield* this.adapter!.stream(messages, options);
  }

  // -----------------------------------------------------------------------
  // Model management
  // -----------------------------------------------------------------------

  /** Switch to a different model. Unloads current, downloads new if needed. */
  async switchModel(modelId: string): Promise<void> {
    if (this.adapter) {
      await this.adapter.unload();
      this.adapter = null;
    }
    this.initPromise = null;
    this.modelId = modelId;
    await this.initialize();
  }

  /** Get current status. */
  getStatus(): ManagerStatus {
    let memoryUsageMb: number | undefined;
    try {
      if (typeof process !== 'undefined' && process.memoryUsage) {
        memoryUsageMb = Math.round(process.memoryUsage().rss / (1024 * 1024));
      }
    } catch {
      // Ignore — memory usage is optional.
    }

    return {
      modelId: this.modelId,
      isModelCached: this.manifest !== null && this.adapter !== null,
      isModelLoaded: this.adapter?.isLoaded() ?? false,
      hardware: this.hardware,
      downloadProgress: this.currentProgress,
      memoryWarning: this.memoryWarning,
      memoryUsageMb,
    };
  }

  /** Unload model and clean up. */
  async shutdown(): Promise<void> {
    this.stopMemoryMonitor();
    if (this.adapter) {
      this.adapter.clearIdleTimeout();
      await this.adapter.unload();
    }
    this.adapter = null;
    this.manifest = null;
    this.hardware = null;
    this.initPromise = null;
    this.currentProgress = null;
    this.memoryWarning = undefined;
    this.progressListeners.clear();
  }

  // -----------------------------------------------------------------------
  // Progress tracking
  // -----------------------------------------------------------------------

  /**
   * Subscribe to download progress events.
   * @returns An unsubscribe function.
   */
  onProgress(callback: ProgressCallback): () => void {
    this.progressListeners.add(callback);
    return () => {
      this.progressListeners.delete(callback);
    };
  }

  // -----------------------------------------------------------------------
  // Internal helpers
  // -----------------------------------------------------------------------

  /**
   * Idempotent lazy initialization. Only the first call triggers `initialize()`;
   * subsequent calls return the same promise.
   */
  private async ensureInitialized(): Promise<void> {
    if (this.adapter?.isLoaded()) return;

    if (!this.initPromise) {
      this.initPromise = this.initialize();
    }

    await this.initPromise;
  }

  private broadcastProgress(progress: ProgressInfo): void {
    for (const listener of this.progressListeners) {
      try {
        listener(progress);
      } catch {
        // Swallow listener errors to avoid breaking the download flow.
      }
    }
  }
}
