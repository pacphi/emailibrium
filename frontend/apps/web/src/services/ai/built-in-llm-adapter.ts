/**
 * BuiltInLlmAdapter — wraps node-llama-cpp v3 behind the LLMProvider interface.
 *
 * Covers BL-1.03 (model loading), BL-1.04 (classification), BL-1.05 (chat).
 */

// ---------------------------------------------------------------------------
// LLMProvider type aliases (mirrored from the canonical provider interface)
// ---------------------------------------------------------------------------

export interface Message {
  role: 'user' | 'assistant' | 'system';
  content: string;
}

export interface CompletionOptions {
  maxTokens?: number;
  temperature?: number;
  topP?: number;
  stopSequences?: string[];
  tools?: unknown[];
}

export interface StreamOptions extends CompletionOptions {
  onToken?: (token: string) => void;
}

export interface Completion {
  content: string;
  finishReason: 'stop' | 'length' | 'tool_use';
  usage: { inputTokens: number; outputTokens: number };
  toolCalls?: unknown[];
}

export interface Token {
  type: 'text' | 'tool_use';
  text?: string;
}

export interface ModelInfo {
  id: string;
  name: string;
  maxTokens: number;
  contextWindow: number;
}

export interface LLMProvider {
  complete(messages: Message[], options?: CompletionOptions): Promise<Completion>;
  stream(messages: Message[], options?: StreamOptions): AsyncGenerator<Token, Completion, void>;
  countTokens(text: string): Promise<number>;
  getModel(): ModelInfo;
  isHealthy(): Promise<boolean>;
}

// ---------------------------------------------------------------------------
// Custom error classes
// ---------------------------------------------------------------------------

export class ModelNotFoundError extends Error {
  constructor(path: string) {
    super(`Model file not found: ${path}`);
    this.name = 'ModelNotFoundError';
  }
}

export class InsufficientMemoryError extends Error {
  constructor(message?: string) {
    super(message ?? 'Insufficient memory to load model');
    this.name = 'InsufficientMemoryError';
  }
}

export class NativeBindingError extends Error {
  constructor(message?: string) {
    super(message ?? 'Failed to initialise native bindings for node-llama-cpp');
    this.name = 'NativeBindingError';
  }
}

// ---------------------------------------------------------------------------
// Classification types
// ---------------------------------------------------------------------------

export interface ClassificationPrompt {
  subject: string;
  sender: string;
  bodyPreview: string;
  categories: string[];
}

export interface ClassificationResult {
  category: string;
  confidence: number;
  reasoning?: string;
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

export interface BuiltInLlmConfig {
  modelPath: string;
  /** Context window size in tokens. @default 2048 */
  contextSize?: number;
  /** Auto-unload after this many ms of inactivity. @default 300_000 */
  idleTimeoutMs?: number;
}

// ---------------------------------------------------------------------------
// Node-llama-cpp type shims (avoids hard import at declaration time)
// ---------------------------------------------------------------------------

/* eslint-disable @typescript-eslint/no-explicit-any */
type LlamaInstance = any;
type LlamaModel = any;
type LlamaContext = any;
/* eslint-enable @typescript-eslint/no-explicit-any */

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

const DEFAULT_CONTEXT_SIZE = 2048;
const DEFAULT_IDLE_TIMEOUT_MS = 300_000;
const DEFAULT_TEMPERATURE = 0.7;
const DEFAULT_MAX_TOKENS = 512;
const CLASSIFICATION_TEMPERATURE = 0.1;
const CLASSIFICATION_MAX_TOKENS = 200;

/**
 * Wraps a local GGUF model loaded via node-llama-cpp, exposing both the
 * `LLMProvider` contract and an email-specific `classify` helper.
 */
export class BuiltInLlmAdapter implements LLMProvider {
  private readonly config: Required<BuiltInLlmConfig>;

  private llama: LlamaInstance | null = null;
  private model: LlamaModel | null = null;
  private context: LlamaContext | null = null;
  private idleTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(config: BuiltInLlmConfig) {
    this.config = {
      modelPath: config.modelPath,
      contextSize: config.contextSize ?? DEFAULT_CONTEXT_SIZE,
      idleTimeoutMs: config.idleTimeoutMs ?? DEFAULT_IDLE_TIMEOUT_MS,
    };
  }

  // -----------------------------------------------------------------------
  // Lifecycle
  // -----------------------------------------------------------------------

  /** Load the model and create an inference context. */
  async load(): Promise<void> {
    try {
      const { getLlama } = await import('node-llama-cpp');
      this.llama = await getLlama();
    } catch (err: unknown) {
      throw new NativeBindingError(err instanceof Error ? err.message : String(err));
    }

    try {
      this.model = await this.llama.loadModel({
        modelPath: this.config.modelPath,
      });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg.includes('ENOENT') || msg.includes('no such file') || msg.includes('not found')) {
        throw new ModelNotFoundError(this.config.modelPath);
      }
      if (msg.includes('memory') || msg.includes('alloc') || msg.includes('mmap')) {
        throw new InsufficientMemoryError(msg);
      }
      throw err;
    }

    this.context = await this.model.createContext({
      contextSize: this.config.contextSize,
    });
  }

  /** Dispose of the model and free native resources. */
  async unload(): Promise<void> {
    this.clearIdleTimeout();
    if (this.model) {
      await this.model.dispose();
    }
    this.model = null;
    this.context = null;
    this.llama = null;
  }

  /** Whether the model and context are currently initialised. */
  isLoaded(): boolean {
    return this.model !== null && this.context !== null;
  }

  /** Return metadata about the loaded model. */
  getModelInfo(): ModelInfo {
    return {
      id: this.config.modelPath,
      name: this.config.modelPath.split('/').pop() ?? 'unknown',
      maxTokens: DEFAULT_MAX_TOKENS,
      contextWindow: this.config.contextSize,
    };
  }

  // -----------------------------------------------------------------------
  // Idle-timeout management
  // -----------------------------------------------------------------------

  /** Cancel any pending idle-unload timer. */
  clearIdleTimeout(): void {
    if (this.idleTimer !== null) {
      clearTimeout(this.idleTimer);
      this.idleTimer = null;
    }
  }

  private resetIdleTimeout(): void {
    this.clearIdleTimeout();
    this.idleTimer = setTimeout(() => {
      void this.unload();
    }, this.config.idleTimeoutMs);
  }

  // -----------------------------------------------------------------------
  // BL-1.04  Classification inference
  // -----------------------------------------------------------------------

  /**
   * Classify an email into one of the supplied categories using a
   * JSON-grammar-constrained prompt.
   */
  async classify(prompt: ClassificationPrompt): Promise<ClassificationResult> {
    this.ensureLoaded();

    const { LlamaChatSession } = await import('node-llama-cpp');

    const systemText = [
      'You are an email classification assistant.',
      'Classify the following email into exactly one of the provided categories.',
      'Respond ONLY with valid JSON matching the schema.',
    ].join(' ');

    const userText = [
      `Subject: ${prompt.subject}`,
      `From: ${prompt.sender}`,
      `Body: ${prompt.bodyPreview}`,
      '',
      `Categories: ${prompt.categories.join(', ')}`,
    ].join('\n');

    const grammar = await this.llama.createGrammarForJsonSchema({
      type: 'object' as const,
      properties: {
        category: { type: 'string' as const, enum: prompt.categories },
        confidence: { type: 'number' as const },
        reasoning: { type: 'string' as const },
      },
      required: ['category', 'confidence'],
    });

    const session = new LlamaChatSession({
      contextSequence: this.context.getSequence(),
    });

    try {
      const raw: string = await session.prompt(`${systemText}\n\n${userText}`, {
        grammar,
        temperature: CLASSIFICATION_TEMPERATURE,
        maxTokens: CLASSIFICATION_MAX_TOKENS,
      });

      this.resetIdleTimeout();

      const parsed: ClassificationResult = JSON.parse(raw) as ClassificationResult;
      return parsed;
    } finally {
      session.dispose?.();
    }
  }

  // -----------------------------------------------------------------------
  // BL-1.05  LLMProvider — complete
  // -----------------------------------------------------------------------

  /** Non-streaming completion. */
  async complete(messages: Message[], options?: CompletionOptions): Promise<Completion> {
    this.ensureLoaded();

    const { LlamaChatSession } = await import('node-llama-cpp');

    const session = new LlamaChatSession({
      contextSequence: this.context.getSequence(),
    });

    const { systemPrefix, userPrompt } = this.flattenMessages(messages);

    try {
      const inputTokenCount = (this.model.tokenize(systemPrefix + userPrompt) as unknown[]).length;

      const response: string = await session.prompt(systemPrefix + userPrompt, {
        temperature: options?.temperature ?? DEFAULT_TEMPERATURE,
        maxTokens: options?.maxTokens ?? DEFAULT_MAX_TOKENS,
        topP: options?.topP,
        trimWhitespaceSuffix: false,
      });

      this.resetIdleTimeout();

      const outputTokenCount = (this.model.tokenize(response) as unknown[]).length;

      return {
        content: response,
        finishReason: 'stop',
        usage: {
          inputTokens: inputTokenCount,
          outputTokens: outputTokenCount,
        },
      };
    } finally {
      session.dispose?.();
    }
  }

  // -----------------------------------------------------------------------
  // BL-1.05  LLMProvider — stream
  // -----------------------------------------------------------------------

  /** Streaming completion via async generator. */
  async *stream(
    messages: Message[],
    options?: StreamOptions,
  ): AsyncGenerator<Token, Completion, void> {
    this.ensureLoaded();

    const { LlamaChatSession } = await import('node-llama-cpp');

    const session = new LlamaChatSession({
      contextSequence: this.context.getSequence(),
    });

    const { systemPrefix, userPrompt } = this.flattenMessages(messages);

    const chunks: string[] = [];
    const inputTokenCount = (this.model.tokenize(systemPrefix + userPrompt) as unknown[]).length;

    // We use a shared buffer that the onTextChunk callback pushes into, and
    // the async generator consumes from.  A simple resolve/promise pair
    // coordinates the two.
    let resolve: ((value: void) => void) | null = null;
    let done = false;

    const waitForChunk = (): Promise<void> =>
      new Promise<void>((r) => {
        resolve = r;
      });

    // Fire off the prompt — do NOT await; we yield tokens as they arrive.
    const promptPromise = session
      .prompt(systemPrefix + userPrompt, {
        temperature: options?.temperature ?? DEFAULT_TEMPERATURE,
        maxTokens: options?.maxTokens ?? DEFAULT_MAX_TOKENS,
        topP: options?.topP,
        trimWhitespaceSuffix: false,
        onTextChunk(text: string) {
          chunks.push(text);
          options?.onToken?.(text);
          resolve?.();
        },
      })
      .then(() => {
        done = true;
        resolve?.();
      });

    try {
      let cursor = 0;
      while (!done || cursor < chunks.length) {
        if (cursor >= chunks.length) {
          await waitForChunk();
        }
        while (cursor < chunks.length) {
          const text = chunks[cursor]!;
          cursor++;
          yield { type: 'text', text };
        }
      }

      await promptPromise;

      this.resetIdleTimeout();

      const fullResponse = chunks.join('');
      const outputTokenCount = (this.model.tokenize(fullResponse) as unknown[]).length;

      return {
        content: fullResponse,
        finishReason: 'stop',
        usage: {
          inputTokens: inputTokenCount,
          outputTokens: outputTokenCount,
        },
      };
    } finally {
      session.dispose?.();
    }
  }

  // -----------------------------------------------------------------------
  // BL-1.05  LLMProvider — countTokens / getModel / isHealthy
  // -----------------------------------------------------------------------

  /** Count the number of tokens in the given text. */
  async countTokens(text: string): Promise<number> {
    this.ensureLoaded();
    const tokens = this.model.tokenize(text) as unknown[];
    return tokens.length;
  }

  /** Alias for `getModelInfo` to satisfy the LLMProvider interface. */
  getModel(): ModelInfo {
    return this.getModelInfo();
  }

  /** Returns `true` when the model is loaded and ready to serve requests. */
  async isHealthy(): Promise<boolean> {
    return this.isLoaded();
  }

  // -----------------------------------------------------------------------
  // Internal helpers
  // -----------------------------------------------------------------------

  /**
   * Collapse an array of `Message` objects into a single prompt string.
   * System messages are prepended to the first user message.
   */
  private flattenMessages(messages: Message[]): {
    systemPrefix: string;
    userPrompt: string;
  } {
    const systemParts: string[] = [];
    const conversationParts: string[] = [];

    for (const msg of messages) {
      if (msg.role === 'system') {
        systemParts.push(msg.content);
      } else {
        const prefix = msg.role === 'user' ? 'User' : 'Assistant';
        conversationParts.push(`${prefix}: ${msg.content}`);
      }
    }

    const systemPrefix = systemParts.length > 0 ? systemParts.join('\n') + '\n\n' : '';

    return { systemPrefix, userPrompt: conversationParts.join('\n') };
  }

  /** Throw if the model has not been loaded yet. */
  private ensureLoaded(): void {
    if (!this.isLoaded()) {
      throw new Error('Model is not loaded. Call load() before using the adapter.');
    }
  }
}
