/**
 * BL-3.05 -- GenerativeRouter
 *
 * Routes generative AI requests to the appropriate provider based on user
 * settings.  Falls back to rule-based keyword classification when the
 * configured provider is unavailable or fails.
 */

import type {
  ClassificationPrompt,
  ClassificationResult,
  Message,
  Completion,
  CompletionOptions,
} from './built-in-llm-adapter';
import type { BuiltInLlmManager } from './built-in-llm-manager';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export type GenerativeProvider = 'none' | 'builtin' | 'local' | 'openai' | 'anthropic';

export interface GenerativeRouterConfig {
  provider: GenerativeProvider;
  builtInModelId?: string;
  ollamaBaseUrl?: string;
  openaiApiKey?: string;
  anthropicApiKey?: string;
}

// ---------------------------------------------------------------------------
// Rule-based fallback
// ---------------------------------------------------------------------------

function ruleBasedClassify(prompt: ClassificationPrompt): ClassificationResult {
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
      return { category, confidence: 0.6, reasoning: 'Rule-based keyword match' };
    }
  }

  return {
    category: prompt.categories[0] ?? 'primary',
    confidence: 0.3,
    reasoning: 'No keyword match — default category',
  };
}

// ---------------------------------------------------------------------------
// Stub result for unimplemented providers
// ---------------------------------------------------------------------------

function stubClassify(providerName: string): ClassificationResult {
  return {
    category: 'uncategorized',
    confidence: 0,
    reasoning: `${providerName} provider not yet implemented`,
  };
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

export class GenerativeRouter {
  private config: GenerativeRouterConfig;
  private manager: BuiltInLlmManager | null = null;

  constructor(config: GenerativeRouterConfig) {
    this.config = { ...config };
  }

  /** Update configuration (e.g., when user changes settings). */
  updateConfig(partial: Partial<GenerativeRouterConfig>): void {
    this.config = { ...this.config, ...partial };
  }

  /** Classify an email. Falls back to rule-based on failure. */
  async classify(prompt: ClassificationPrompt): Promise<ClassificationResult> {
    switch (this.config.provider) {
      case 'none':
        return ruleBasedClassify(prompt);

      case 'builtin':
        try {
          const mgr = await this.getManager();
          return await mgr.classify(prompt);
        } catch {
          return ruleBasedClassify(prompt);
        }

      case 'local':
        return stubClassify('Ollama');
      case 'openai':
        return stubClassify('OpenAI');
      case 'anthropic':
        return stubClassify('Anthropic');

      default:
        return ruleBasedClassify(prompt);
    }
  }

  /** Chat completion. Throws if no generative provider configured. */
  async chat(messages: Message[], options?: CompletionOptions): Promise<Completion> {
    switch (this.config.provider) {
      case 'none':
        throw new Error('No generative provider configured.');

      case 'builtin': {
        const mgr = await this.getManager();
        return mgr.chat(messages, options);
      }

      case 'local':
      case 'openai':
      case 'anthropic':
        throw new Error(`${this.config.provider} provider not yet implemented.`);

      default:
        throw new Error('No generative provider configured.');
    }
  }

  /** Get the active provider name. */
  getActiveProvider(): GenerativeProvider {
    return this.config.provider;
  }

  /** Shut down any loaded resources. */
  async shutdown(): Promise<void> {
    if (this.manager) {
      await this.manager.shutdown();
      this.manager = null;
    }
  }

  // -------------------------------------------------------------------------
  // Internal
  // -------------------------------------------------------------------------

  private async getManager(): Promise<BuiltInLlmManager> {
    if (!this.manager) {
      const { BuiltInLlmManager: Ctor } = await import('./built-in-llm-manager');
      this.manager = new Ctor({ modelId: this.config.builtInModelId });
    }
    return this.manager;
  }
}
