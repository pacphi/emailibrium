import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { GenerativeRouterConfig } from '../generative-router';
import type { ClassificationPrompt } from '../built-in-llm-adapter';

// ---------------------------------------------------------------------------
// Mock BuiltInLlmManager (vitest 4: hoist mock variables)
// ---------------------------------------------------------------------------

const { mockClassify, mockChat, mockShutdown } = vi.hoisted(() => ({
  mockClassify: vi.fn(),
  mockChat: vi.fn(),
  mockShutdown: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('../built-in-llm-manager', () => ({
  BuiltInLlmManager: vi.fn().mockImplementation(function () {
    return {
      classify: mockClassify,
      chat: mockChat,
      shutdown: mockShutdown,
    };
  }),
}));

import { GenerativeRouter } from '../generative-router';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('GenerativeRouter', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // -----------------------------------------------------------------------
  // Rule-based classification
  // -----------------------------------------------------------------------

  describe('rule-based classify (provider=none)', () => {
    it('returns keyword-matched category for invoice-related email', async () => {
      const router = new GenerativeRouter(makeConfig());
      const result = await router.classify(makePrompt({ subject: 'Your invoice is ready' }));

      expect(result.category).toBe('updates');
      expect(result.confidence).toBe(0.6);
      expect(result.reasoning).toBe('Rule-based keyword match');
    });

    it('returns keyword-matched category for promotional email', async () => {
      const router = new GenerativeRouter(makeConfig());
      const result = await router.classify(makePrompt({ subject: 'Big sale this weekend!' }));

      expect(result.category).toBe('promotions');
      expect(result.confidence).toBe(0.6);
    });

    it('returns keyword-matched category for meeting invite', async () => {
      const router = new GenerativeRouter(makeConfig());
      const result = await router.classify(makePrompt({ subject: 'Meeting tomorrow at 3pm' }));

      expect(result.category).toBe('updates');
    });

    it('returns keyword-matched category for GitHub notification', async () => {
      const router = new GenerativeRouter(makeConfig());
      const result = await router.classify(makePrompt({ sender: 'noreply@github.com' }));

      expect(result.category).toBe('updates');
    });

    it('returns default category when no keyword matches', async () => {
      const router = new GenerativeRouter(makeConfig());
      const result = await router.classify(makePrompt());

      expect(result.category).toBe('primary');
      expect(result.confidence).toBe(0.3);
      expect(result.reasoning).toBe('No keyword match — default category');
    });

    it('uses first category from prompt as default', async () => {
      const router = new GenerativeRouter(makeConfig());
      const result = await router.classify(makePrompt({ categories: ['inbox', 'spam'] }));

      expect(result.category).toBe('inbox');
    });
  });

  // -----------------------------------------------------------------------
  // Builtin provider
  // -----------------------------------------------------------------------

  describe('builtin provider', () => {
    it('lazy-initializes BuiltInLlmManager and delegates classify', async () => {
      const expected = { category: 'updates', confidence: 0.95, reasoning: 'LLM' };
      mockClassify.mockResolvedValueOnce(expected);

      const router = new GenerativeRouter(
        makeConfig({ provider: 'builtin', builtInModelId: 'test-model' }),
      );
      const result = await router.classify(makePrompt());

      expect(result).toEqual(expected);
      expect(mockClassify).toHaveBeenCalledOnce();
    });

    it('falls back to rule-based when BuiltInLlmManager.classify throws', async () => {
      mockClassify.mockRejectedValueOnce(new Error('model load failed'));

      const router = new GenerativeRouter(makeConfig({ provider: 'builtin' }));
      const result = await router.classify(makePrompt({ subject: 'Your receipt from Apple' }));

      expect(result.category).toBe('updates');
      expect(result.confidence).toBe(0.6);
      expect(result.reasoning).toBe('Rule-based keyword match');
    });

    it('delegates chat to BuiltInLlmManager', async () => {
      const expected = {
        content: 'Hello!',
        finishReason: 'stop' as const,
        usage: { inputTokens: 5, outputTokens: 3 },
      };
      mockChat.mockResolvedValueOnce(expected);

      const router = new GenerativeRouter(makeConfig({ provider: 'builtin' }));
      const messages = [{ role: 'user' as const, content: 'Hi' }];
      const result = await router.chat(messages);

      expect(result).toEqual(expected);
      expect(mockChat).toHaveBeenCalledWith(messages, undefined);
    });
  });

  // -----------------------------------------------------------------------
  // Stub providers
  // -----------------------------------------------------------------------

  describe('stub providers', () => {
    it.each(['local', 'openai', 'anthropic'] as const)(
      '%s returns stub classification result',
      async (provider) => {
        const router = new GenerativeRouter(makeConfig({ provider }));
        const result = await router.classify(makePrompt());

        expect(result.category).toBe('uncategorized');
        expect(result.confidence).toBe(0);
        expect(result.reasoning).toMatch(/not yet implemented/);
      },
    );

    it.each(['local', 'openai', 'anthropic'] as const)('%s throws on chat', async (provider) => {
      const router = new GenerativeRouter(makeConfig({ provider }));

      await expect(router.chat([{ role: 'user', content: 'hi' }])).rejects.toThrow(
        /not yet implemented/,
      );
    });
  });

  // -----------------------------------------------------------------------
  // Provider=none chat
  // -----------------------------------------------------------------------

  describe('provider=none chat', () => {
    it('throws "No generative provider configured"', async () => {
      const router = new GenerativeRouter(makeConfig());

      await expect(router.chat([{ role: 'user', content: 'hi' }])).rejects.toThrow(
        'No generative provider configured.',
      );
    });
  });

  // -----------------------------------------------------------------------
  // updateConfig
  // -----------------------------------------------------------------------

  describe('updateConfig', () => {
    it('changes the active provider', () => {
      const router = new GenerativeRouter(makeConfig());
      expect(router.getActiveProvider()).toBe('none');

      router.updateConfig({ provider: 'builtin' });
      expect(router.getActiveProvider()).toBe('builtin');
    });

    it('preserves existing config fields when partially updating', async () => {
      const router = new GenerativeRouter(
        makeConfig({ provider: 'builtin', builtInModelId: 'model-a' }),
      );
      router.updateConfig({ provider: 'none' });

      expect(router.getActiveProvider()).toBe('none');
      // classify still works (uses rule-based now)
      const result = await router.classify(makePrompt());
      expect(result.confidence).toBeLessThanOrEqual(0.6);
    });
  });

  // -----------------------------------------------------------------------
  // getActiveProvider
  // -----------------------------------------------------------------------

  describe('getActiveProvider', () => {
    it('returns the current provider', () => {
      const router = new GenerativeRouter(makeConfig({ provider: 'openai' }));
      expect(router.getActiveProvider()).toBe('openai');
    });
  });

  // -----------------------------------------------------------------------
  // shutdown
  // -----------------------------------------------------------------------

  describe('shutdown', () => {
    it('calls manager.shutdown when manager was initialized', async () => {
      mockClassify.mockResolvedValueOnce({ category: 'a', confidence: 1 });

      const router = new GenerativeRouter(makeConfig({ provider: 'builtin' }));
      // Force lazy init by classifying
      await router.classify(makePrompt());

      await router.shutdown();
      expect(mockShutdown).toHaveBeenCalledOnce();
    });

    it('does nothing when no manager was created', async () => {
      const router = new GenerativeRouter(makeConfig());
      await router.shutdown(); // should not throw
      expect(mockShutdown).not.toHaveBeenCalled();
    });
  });
});
