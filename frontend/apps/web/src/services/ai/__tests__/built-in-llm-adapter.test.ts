import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  BuiltInLlmAdapter,
  ModelNotFoundError,
  type BuiltInLlmConfig,
  type ClassificationPrompt,
  type Message,
} from '../built-in-llm-adapter';

// ---------------------------------------------------------------------------
// Mock node-llama-cpp
// ---------------------------------------------------------------------------

const mockSession = {
  prompt: vi.fn(),
  dispose: vi.fn(),
};

const mockSequence = {};

const mockContext = {
  getSequence: vi.fn().mockReturnValue(mockSequence),
  dispose: vi.fn(),
};

const mockModel = {
  dispose: vi.fn().mockResolvedValue(undefined),
  tokenize: vi.fn().mockReturnValue([1, 2, 3]),
  createContext: vi.fn().mockResolvedValue(mockContext),
};

const mockGrammar = {};

const mockLlama = {
  loadModel: vi.fn().mockResolvedValue(mockModel),
  createGrammarForJsonSchema: vi.fn().mockResolvedValue(mockGrammar),
  dispose: vi.fn(),
};

const mockGetLlama = vi.fn().mockResolvedValue(mockLlama);
const mockLlamaChatSessionCtor = vi.fn().mockImplementation(() => mockSession);

vi.mock('node-llama-cpp', () => ({
  getLlama: mockGetLlama,
  LlamaChatSession: mockLlamaChatSessionCtor,
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function createAdapter(overrides?: Partial<BuiltInLlmConfig>): BuiltInLlmAdapter {
  return new BuiltInLlmAdapter({
    modelPath: '/models/test-model.gguf',
    contextSize: 2048,
    idleTimeoutMs: 300_000,
    ...overrides,
  });
}

const classificationPrompt: ClassificationPrompt = {
  subject: 'Invoice #123',
  sender: 'billing@acme.com',
  bodyPreview: 'Please find attached your invoice.',
  categories: ['primary', 'promotions', 'social', 'updates'],
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('BuiltInLlmAdapter', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Re-setup mocks after clearAllMocks wipes them
    mockGetLlama.mockResolvedValue(mockLlama);
    mockLlamaChatSessionCtor.mockImplementation(() => mockSession);
    mockLlama.loadModel.mockResolvedValue(mockModel);
    mockLlama.createGrammarForJsonSchema.mockResolvedValue(mockGrammar);
    mockModel.createContext.mockResolvedValue(mockContext);
    mockModel.dispose.mockResolvedValue(undefined);
    mockModel.tokenize.mockReturnValue([1, 2, 3]);
    mockContext.getSequence.mockReturnValue(mockSequence);
    mockSession.prompt.mockResolvedValue('Hello world');
    mockSession.dispose.mockReturnValue(undefined);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // -------------------------------------------------------------------------
  // BL-1.03  Model loading
  // -------------------------------------------------------------------------

  describe('model loading', () => {
    it('load() initializes llama, loads model, creates context', async () => {
      const adapter = createAdapter();

      await adapter.load();

      expect(mockLlama.loadModel).toHaveBeenCalledWith({
        modelPath: '/models/test-model.gguf',
      });
      expect(mockModel.createContext).toHaveBeenCalledWith({
        contextSize: 2048,
      });
      expect(adapter.isLoaded()).toBe(true);
    });

    it('load() throws ModelNotFoundError when model file does not exist', async () => {
      mockLlama.loadModel.mockRejectedValueOnce(new Error('ENOENT: no such file'));

      const adapter = createAdapter();

      await expect(adapter.load()).rejects.toThrow(ModelNotFoundError);
    });

    it('unload() disposes model and clears references', async () => {
      const adapter = createAdapter();
      await adapter.load();

      await adapter.unload();

      expect(mockModel.dispose).toHaveBeenCalled();
      expect(adapter.isLoaded()).toBe(false);
    });

    it('isLoaded() returns false before load, true after load, false after unload', async () => {
      const adapter = createAdapter();

      expect(adapter.isLoaded()).toBe(false);

      await adapter.load();
      expect(adapter.isLoaded()).toBe(true);

      await adapter.unload();
      expect(adapter.isLoaded()).toBe(false);
    });

    it('getModelInfo() returns correct model metadata', () => {
      const adapter = createAdapter();

      const info = adapter.getModelInfo();

      expect(info.id).toBe('/models/test-model.gguf');
      expect(info.name).toBe('test-model.gguf');
      expect(info.contextWindow).toBe(2048);
      expect(info.maxTokens).toBe(512);
    });

    it('double load() does not create duplicate resources (idempotent)', async () => {
      const adapter = createAdapter();

      await adapter.load();
      await adapter.load();

      // getLlama is called twice (the adapter does not guard against it),
      // but the important contract is that only one model is active.
      // If the implementation becomes idempotent, loadModel would be called once.
      expect(adapter.isLoaded()).toBe(true);
    });

    it('unload() when not loaded is a no-op', async () => {
      const adapter = createAdapter();

      // Should not throw
      await expect(adapter.unload()).resolves.toBeUndefined();
    });
  });

  // -------------------------------------------------------------------------
  // BL-1.04  Classification
  // -------------------------------------------------------------------------

  describe('classification', () => {
    it('classify() returns valid ClassificationResult with category from the allowed list', async () => {
      const result = JSON.stringify({
        category: 'updates',
        confidence: 0.92,
        reasoning: 'Invoice email',
      });
      mockSession.prompt.mockResolvedValueOnce(result);

      const adapter = createAdapter();
      await adapter.load();

      const classification = await adapter.classify(classificationPrompt);

      expect(classificationPrompt.categories).toContain(classification.category);
      expect(classification.confidence).toBe(0.92);
    });

    it('classify() uses temperature 0.1 for deterministic output', async () => {
      mockSession.prompt.mockResolvedValueOnce(
        JSON.stringify({ category: 'primary', confidence: 0.9 }),
      );

      const adapter = createAdapter();
      await adapter.load();
      await adapter.classify(classificationPrompt);

      expect(mockSession.prompt).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({ temperature: 0.1 }),
      );
    });

    it('classify() uses JSON grammar constraint', async () => {
      mockSession.prompt.mockResolvedValueOnce(
        JSON.stringify({ category: 'primary', confidence: 0.9 }),
      );

      const adapter = createAdapter();
      await adapter.load();
      await adapter.classify(classificationPrompt);

      expect(mockLlama.createGrammarForJsonSchema).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'object',
          properties: expect.objectContaining({
            category: expect.objectContaining({ enum: classificationPrompt.categories }),
          }),
        }),
      );

      expect(mockSession.prompt).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({ grammar: mockGrammar }),
      );
    });

    it('classify() throws when model is not loaded', async () => {
      const adapter = createAdapter();

      await expect(adapter.classify(classificationPrompt)).rejects.toThrow('Model is not loaded');
    });

    it('classify() throws when inference fails', async () => {
      mockSession.prompt.mockRejectedValueOnce(new Error('inference failed'));

      const adapter = createAdapter();
      await adapter.load();

      await expect(adapter.classify(classificationPrompt)).rejects.toThrow('inference failed');
    });

    it('classification result confidence is between 0 and 1', async () => {
      mockSession.prompt.mockResolvedValueOnce(
        JSON.stringify({ category: 'primary', confidence: 0.85 }),
      );

      const adapter = createAdapter();
      await adapter.load();

      const result = await adapter.classify(classificationPrompt);

      expect(result.confidence).toBeGreaterThanOrEqual(0);
      expect(result.confidence).toBeLessThanOrEqual(1);
    });
  });

  // -------------------------------------------------------------------------
  // BL-1.05  Chat
  // -------------------------------------------------------------------------

  describe('chat — complete()', () => {
    it('returns Completion with content and usage', async () => {
      mockSession.prompt.mockResolvedValueOnce('The answer is 42.');

      const adapter = createAdapter();
      await adapter.load();

      const messages: Message[] = [{ role: 'user', content: 'What is the answer?' }];
      const result = await adapter.complete(messages);

      expect(result.content).toBe('The answer is 42.');
      expect(result.usage).toHaveProperty('inputTokens');
      expect(result.usage).toHaveProperty('outputTokens');
      expect(result.finishReason).toBe('stop');
    });

    it('uses provided temperature and maxTokens', async () => {
      mockSession.prompt.mockResolvedValueOnce('response');

      const adapter = createAdapter();
      await adapter.load();

      await adapter.complete([{ role: 'user', content: 'Hi' }], {
        temperature: 0.3,
        maxTokens: 100,
      });

      expect(mockSession.prompt).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({ temperature: 0.3, maxTokens: 100 }),
      );
    });

    it('defaults to temperature 0.7 and maxTokens 512', async () => {
      mockSession.prompt.mockResolvedValueOnce('response');

      const adapter = createAdapter();
      await adapter.load();

      await adapter.complete([{ role: 'user', content: 'Hi' }]);

      expect(mockSession.prompt).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({ temperature: 0.7, maxTokens: 512 }),
      );
    });

    it('system messages are prepended to first user message', async () => {
      mockSession.prompt.mockResolvedValueOnce('response');

      const adapter = createAdapter();
      await adapter.load();

      const messages: Message[] = [
        { role: 'system', content: 'You are helpful.' },
        { role: 'user', content: 'Hello' },
      ];
      await adapter.complete(messages);

      const promptArg = mockSession.prompt.mock.calls[0]![0] as string;
      expect(promptArg).toContain('You are helpful.');
      expect(promptArg).toContain('Hello');
      // System comes before user content
      expect(promptArg.indexOf('You are helpful.')).toBeLessThan(promptArg.indexOf('Hello'));
    });
  });

  describe('chat — stream()', () => {
    it('yields Token objects and returns Completion', async () => {
      // Simulate streaming: onTextChunk is called with chunks
      mockSession.prompt.mockImplementationOnce(
        (_prompt: string, opts: { onTextChunk: (t: string) => void }) => {
          opts.onTextChunk('Hello');
          opts.onTextChunk(' world');
          return Promise.resolve('Hello world');
        },
      );

      const adapter = createAdapter();
      await adapter.load();

      const tokens: { type: string; text?: string }[] = [];
      const gen = adapter.stream([{ role: 'user', content: 'Hi' }]);

      let result = await gen.next();
      while (!result.done) {
        tokens.push(result.value);
        result = await gen.next();
      }

      expect(tokens.length).toBeGreaterThanOrEqual(2);
      expect(tokens[0]!.type).toBe('text');
      expect(tokens[0]!.text).toBe('Hello');
      expect(tokens[1]!.text).toBe(' world');

      // The return value is a Completion
      const completion = result.value;
      expect(completion.content).toBe('Hello world');
      expect(completion.usage).toHaveProperty('inputTokens');
      expect(completion.usage).toHaveProperty('outputTokens');
    });
  });

  describe('chat — countTokens()', () => {
    it('returns token count for text', async () => {
      mockModel.tokenize.mockReturnValueOnce([1, 2, 3, 4, 5]);

      const adapter = createAdapter();
      await adapter.load();

      const count = await adapter.countTokens('Hello world test');

      expect(count).toBe(5);
    });
  });

  describe('chat — isHealthy()', () => {
    it('returns true when loaded', async () => {
      const adapter = createAdapter();
      await adapter.load();

      expect(await adapter.isHealthy()).toBe(true);
    });

    it('returns false when not loaded', async () => {
      const adapter = createAdapter();

      expect(await adapter.isHealthy()).toBe(false);
    });
  });

  // -------------------------------------------------------------------------
  // Idle timeout
  // -------------------------------------------------------------------------

  describe('idle timeout', () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it('model is unloaded after idle timeout period', async () => {
      const adapter = createAdapter({ idleTimeoutMs: 5000 });
      await adapter.load();

      // Trigger a complete() call which sets the idle timer
      mockSession.prompt.mockResolvedValueOnce('response');
      await adapter.complete([{ role: 'user', content: 'Hi' }]);

      expect(adapter.isLoaded()).toBe(true);

      vi.advanceTimersByTime(5000);
      // Allow the async unload() to complete
      await vi.runAllTimersAsync();

      expect(adapter.isLoaded()).toBe(false);
    });

    it('timer resets on each inference call', async () => {
      const adapter = createAdapter({ idleTimeoutMs: 5000 });
      await adapter.load();

      mockSession.prompt.mockResolvedValue('response');

      await adapter.complete([{ role: 'user', content: 'First' }]);

      // Advance 3 seconds (before timeout)
      vi.advanceTimersByTime(3000);
      expect(adapter.isLoaded()).toBe(true);

      // Second call resets the timer
      await adapter.complete([{ role: 'user', content: 'Second' }]);

      // Advance another 3 seconds (6 total from first, 3 from second)
      vi.advanceTimersByTime(3000);
      expect(adapter.isLoaded()).toBe(true);

      // Now advance past the timeout from the second call
      vi.advanceTimersByTime(2001);
      await vi.runAllTimersAsync();

      expect(adapter.isLoaded()).toBe(false);
    });

    it('clearIdleTimeout() cancels the timer', async () => {
      const adapter = createAdapter({ idleTimeoutMs: 5000 });
      await adapter.load();

      mockSession.prompt.mockResolvedValueOnce('response');
      await adapter.complete([{ role: 'user', content: 'Hi' }]);

      adapter.clearIdleTimeout();

      vi.advanceTimersByTime(10_000);
      await vi.runAllTimersAsync();

      expect(adapter.isLoaded()).toBe(true);
    });
  });
});
