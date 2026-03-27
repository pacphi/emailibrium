import { describe, it, expect, vi, beforeEach } from 'vitest';

// ---------------------------------------------------------------------------
// Provide localStorage BEFORE any module that uses it gets evaluated.
// vi.hoisted() runs before all imports, ensuring Zustand's persist
// middleware can access localStorage.setItem during store creation.
// ---------------------------------------------------------------------------

vi.hoisted(() => {
  const map = new Map<string, string>();
  const storage = {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
    removeItem: (key: string) => {
      map.delete(key);
    },
    clear: () => {
      map.clear();
    },
    get length() {
      return map.size;
    },
    key: (index: number) => [...map.keys()][index] ?? null,
  };
  Object.defineProperty(globalThis, 'localStorage', {
    value: storage,
    writable: true,
    configurable: true,
  });
});

import { useSettings } from '../useSettings';

/**
 * Tests for the useSettings Zustand store.
 * Tested directly via getState()/setState() — no renderHook needed.
 */

beforeEach(() => {
  (globalThis.localStorage as Storage).clear();
  useSettings.getState().resetAll();
});

describe('useSettings store', () => {
  it('has correct default state', () => {
    const state = useSettings.getState();

    expect(state.defaultComposeAccountId).toBeNull();
    expect(state.notificationsEnabled).toBe(true);
    expect(state.syncFrequencyMinutes).toBe(5);
    expect(state.embeddingModel).toBe('all-MiniLM-L6-v2');
    expect(state.llmProvider).toBe('builtin');
    expect(state.builtInLlmModel).toBe('qwen2.5-0.5b-q4km');
    expect(state.builtInLlmIdleTimeout).toBe(300);
    expect(state.builtInLlmMaxContext).toBe(2048);
    expect(state.builtInLlmTemperature).toBe(0.7);
    expect(state.openaiApiKey).toBe('');
    expect(state.anthropicApiKey).toBe('');
    expect(state.ollamaBaseUrl).toBe('http://localhost:11434');
    expect(state.sonaLearningEnabled).toBe(true);
    expect(state.learningRateSensitivity).toBe(0.5);
    expect(state.encryptionAtRest).toBe(false);
    expect(state.masterPasswordHash).toBeNull();
    expect(state.dataRetentionDays).toBe(90);
    expect(state.theme).toBe('system');
    expect(state.sidebarPosition).toBe('left');
    expect(state.emailListDensity).toBe('comfortable');
    expect(state.fontSize).toBe(14);
  });

  it('sets LLM provider', () => {
    useSettings.getState().setLlmProvider('openai');
    expect(useSettings.getState().llmProvider).toBe('openai');

    useSettings.getState().setLlmProvider('anthropic');
    expect(useSettings.getState().llmProvider).toBe('anthropic');
  });

  it('sets built-in LLM model and temperature', () => {
    useSettings.getState().setBuiltInLlmModel('llama-3.2-1b');
    expect(useSettings.getState().builtInLlmModel).toBe('llama-3.2-1b');

    useSettings.getState().setBuiltInLlmTemperature(0.3);
    expect(useSettings.getState().builtInLlmTemperature).toBe(0.3);
  });

  it('sets API keys', () => {
    useSettings.getState().setOpenaiApiKey('sk-test-key');
    expect(useSettings.getState().openaiApiKey).toBe('sk-test-key');

    useSettings.getState().setAnthropicApiKey('sk-ant-test');
    expect(useSettings.getState().anthropicApiKey).toBe('sk-ant-test');
  });

  it('sets appearance options', () => {
    useSettings.getState().setTheme('dark');
    expect(useSettings.getState().theme).toBe('dark');

    useSettings.getState().setSidebarPosition('right');
    expect(useSettings.getState().sidebarPosition).toBe('right');

    useSettings.getState().setEmailListDensity('compact');
    expect(useSettings.getState().emailListDensity).toBe('compact');

    useSettings.getState().setFontSize(18);
    expect(useSettings.getState().fontSize).toBe(18);
  });

  it('sets general options', () => {
    useSettings.getState().setDefaultComposeAccountId('acc-123');
    expect(useSettings.getState().defaultComposeAccountId).toBe('acc-123');

    useSettings.getState().setNotificationsEnabled(false);
    expect(useSettings.getState().notificationsEnabled).toBe(false);

    useSettings.getState().setSyncFrequencyMinutes(15);
    expect(useSettings.getState().syncFrequencyMinutes).toBe(15);
  });

  it('sets privacy options', () => {
    useSettings.getState().setEncryptionAtRest(true);
    expect(useSettings.getState().encryptionAtRest).toBe(true);

    useSettings.getState().setMasterPasswordHash('abc123hash');
    expect(useSettings.getState().masterPasswordHash).toBe('abc123hash');

    useSettings.getState().setDataRetentionDays(30);
    expect(useSettings.getState().dataRetentionDays).toBe(30);
  });

  it('sets learning options', () => {
    useSettings.getState().setSonaLearningEnabled(false);
    expect(useSettings.getState().sonaLearningEnabled).toBe(false);

    useSettings.getState().setLearningRateSensitivity(0.9);
    expect(useSettings.getState().learningRateSensitivity).toBe(0.9);
  });

  it('resetAll restores all defaults', () => {
    const s = useSettings.getState();
    s.setLlmProvider('openai');
    s.setTheme('dark');
    s.setFontSize(20);
    s.setOpenaiApiKey('sk-key');
    s.setNotificationsEnabled(false);

    expect(useSettings.getState().llmProvider).toBe('openai');
    expect(useSettings.getState().fontSize).toBe(20);

    useSettings.getState().resetAll();

    const reset = useSettings.getState();
    expect(reset.llmProvider).toBe('builtin');
    expect(reset.theme).toBe('system');
    expect(reset.fontSize).toBe(14);
    expect(reset.openaiApiKey).toBe('');
    expect(reset.notificationsEnabled).toBe(true);
  });

  it('partialize excludes action functions from persisted state', () => {
    const persistApi = (
      useSettings as unknown as {
        persist: { getOptions: () => { partialize: (s: unknown) => Record<string, unknown> } };
      }
    ).persist;
    const partialized = persistApi.getOptions().partialize(useSettings.getState());

    expect(partialized).toHaveProperty('llmProvider');
    expect(partialized).toHaveProperty('theme');
    expect(partialized).toHaveProperty('fontSize');

    expect(partialized).not.toHaveProperty('setLlmProvider');
    expect(partialized).not.toHaveProperty('setTheme');
    expect(partialized).not.toHaveProperty('resetAll');
  });

  it('migration from v0 sets llmProvider to builtin when missing or none', () => {
    const persistApi = (
      useSettings as unknown as {
        persist: { getOptions: () => { migrate: (state: unknown, version: number) => unknown } };
      }
    ).persist;
    const { migrate } = persistApi.getOptions();

    const v0None = { llmProvider: 'none', theme: 'dark' };
    const migratedNone = migrate(v0None, 0) as Record<string, unknown>;
    expect(migratedNone.llmProvider).toBe('builtin');

    const v0Missing = { theme: 'light' };
    const migratedMissing = migrate(v0Missing, 0) as Record<string, unknown>;
    expect(migratedMissing.llmProvider).toBe('builtin');

    const v0Valid = { llmProvider: 'openai', theme: 'dark' };
    const migratedValid = migrate(v0Valid, 0) as Record<string, unknown>;
    expect(migratedValid.llmProvider).toBe('openai');
  });
});
