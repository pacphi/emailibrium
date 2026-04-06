import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';

export type Theme = 'light' | 'dark' | 'system';
export type SidebarPosition = 'left' | 'right';
export type EmailListDensity = 'compact' | 'comfortable' | 'spacious';
export type LlmProvider = 'none' | 'builtin' | 'local' | 'openai' | 'anthropic';

// Keys that are safe to persist to the backend (excludes secrets and actions).
const BACKEND_PERSISTED_KEYS: readonly string[] = [
  'defaultComposeAccountId',
  'notificationsEnabled',
  'syncFrequencyMinutes',
  'embeddingModel',
  'llmProvider',
  'builtInLlmModel',
  'builtInLlmIdleTimeout',
  'builtInLlmMaxContext',
  'builtInLlmTemperature',
  'ollamaBaseUrl',
  'sonaLearningEnabled',
  'learningRateSensitivity',
  'encryptionAtRest',
  'dataRetentionDays',
  'theme',
  'sidebarPosition',
  'emailListDensity',
  'fontSize',
] as const;

/** Push settings to the backend for cross-restart persistence. */
let syncTimer: ReturnType<typeof setTimeout> | null = null;

function syncToBackend(state: Record<string, unknown>): void {
  // Debounce: wait 500ms after last change to batch rapid updates.
  if (syncTimer) clearTimeout(syncTimer);
  syncTimer = setTimeout(() => {
    const payload: Record<string, unknown> = {};
    for (const key of BACKEND_PERSISTED_KEYS) {
      if (key in state) {
        payload[key] = state[key];
      }
    }
    fetch('/api/v1/ai/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    }).catch(() => {
      /* Backend may be unavailable — localStorage is the primary store. */
    });
  }, 500);
}

/**
 * Bidirectional sync with the backend on first load.
 *
 * - If the backend has no settings yet (fresh DB / new migration),
 *   push the current localStorage settings to the backend so the
 *   server can restore them on next restart.
 * - If the backend has settings, merge them into the local store
 *   for any keys the user hasn't changed from defaults locally.
 */
export async function hydrateFromBackend(): Promise<void> {
  try {
    const res = await fetch('/api/v1/ai/settings');
    if (!res.ok) return;
    const remote: Record<string, string> = await res.json();

    const current = useSettings.getState();

    // Backend is empty — push current local settings to seed it.
    if (!remote || Object.keys(remote).length === 0) {
      syncToBackend(current as unknown as Record<string, unknown>);
      return;
    }

    // Backend has settings — merge into local store for keys still at defaults.
    const updates: Record<string, unknown> = {};
    for (const [key, val] of Object.entries(remote)) {
      if (!(key in current)) continue;
      const currentVal = (current as unknown as Record<string, unknown>)[key];
      const defaultVal = (DEFAULT_STATE as unknown as Record<string, unknown>)[key];
      if (currentVal === defaultVal && val !== String(defaultVal)) {
        if (val === 'true') updates[key] = true;
        else if (val === 'false') updates[key] = false;
        else if (val === 'null') updates[key] = null;
        else if (/^-?\d+(\.\d+)?$/.test(val)) updates[key] = Number(val);
        else updates[key] = val;
      }
    }
    if (Object.keys(updates).length > 0) {
      useSettings.setState(updates);
    }
  } catch {
    /* Backend unavailable — use localStorage only. */
  }
}

export interface SettingsState {
  // General
  defaultComposeAccountId: string | null;
  notificationsEnabled: boolean;
  syncFrequencyMinutes: number;

  // AI / LLM
  embeddingModel: string;
  llmProvider: LlmProvider;
  builtInLlmModel: string;
  builtInLlmIdleTimeout: number;
  builtInLlmMaxContext: number;
  builtInLlmTemperature: number;
  openaiApiKey: string;
  anthropicApiKey: string;
  ollamaBaseUrl: string;
  sonaLearningEnabled: boolean;
  learningRateSensitivity: number;

  // Privacy
  encryptionAtRest: boolean;
  masterPasswordHash: string | null;
  dataRetentionDays: number;

  // Appearance
  theme: Theme;
  sidebarPosition: SidebarPosition;
  emailListDensity: EmailListDensity;
  fontSize: number;

  // Actions
  setDefaultComposeAccountId: (id: string | null) => void;
  setNotificationsEnabled: (enabled: boolean) => void;
  setSyncFrequencyMinutes: (minutes: number) => void;
  setEmbeddingModel: (model: string) => void;
  setLlmProvider: (provider: LlmProvider) => void;
  setBuiltInLlmModel: (model: string) => void;
  setBuiltInLlmIdleTimeout: (timeout: number) => void;
  setBuiltInLlmMaxContext: (context: number) => void;
  setBuiltInLlmTemperature: (temp: number) => void;
  setOpenaiApiKey: (key: string) => void;
  setAnthropicApiKey: (key: string) => void;
  setOllamaBaseUrl: (url: string) => void;
  setSonaLearningEnabled: (enabled: boolean) => void;
  setLearningRateSensitivity: (rate: number) => void;
  setEncryptionAtRest: (enabled: boolean) => void;
  setMasterPasswordHash: (hash: string | null) => void;
  setDataRetentionDays: (days: number) => void;
  setTheme: (theme: Theme) => void;
  setSidebarPosition: (position: SidebarPosition) => void;
  setEmailListDensity: (density: EmailListDensity) => void;
  setFontSize: (size: number) => void;
  resetAll: () => void;
}

const DEFAULT_STATE = {
  defaultComposeAccountId: null,
  notificationsEnabled: true,
  syncFrequencyMinutes: 5,
  embeddingModel: 'all-MiniLM-L6-v2',
  llmProvider: 'builtin' as LlmProvider,
  builtInLlmModel: 'qwen3-1.7b-q4km',
  builtInLlmIdleTimeout: 300,
  builtInLlmMaxContext: 2048,
  builtInLlmTemperature: 0.7,
  openaiApiKey: '',
  anthropicApiKey: '',
  ollamaBaseUrl: 'http://localhost:11434',
  sonaLearningEnabled: true,
  learningRateSensitivity: 0.5,
  encryptionAtRest: false,
  masterPasswordHash: null,
  dataRetentionDays: 90,
  theme: 'system' as Theme,
  sidebarPosition: 'left' as SidebarPosition,
  emailListDensity: 'comfortable' as EmailListDensity,
  fontSize: 14,
};

/**
 * Zustand store for application settings, persisted to localStorage
 * via the secureStorage key. In production, swap createJSONStorage
 * for a wrapper around the secure electron/tauri storage API.
 */
export const useSettings = create<SettingsState>()(
  persist(
    (set) => ({
      ...DEFAULT_STATE,

      setDefaultComposeAccountId: (id) => set({ defaultComposeAccountId: id }),
      setNotificationsEnabled: (enabled) => set({ notificationsEnabled: enabled }),
      setSyncFrequencyMinutes: (minutes) => set({ syncFrequencyMinutes: minutes }),
      setEmbeddingModel: (model) => set({ embeddingModel: model }),
      setLlmProvider: (provider) => set({ llmProvider: provider }),
      setBuiltInLlmModel: (model) => set({ builtInLlmModel: model }),
      setBuiltInLlmIdleTimeout: (timeout) => set({ builtInLlmIdleTimeout: timeout }),
      setBuiltInLlmMaxContext: (context) => set({ builtInLlmMaxContext: context }),
      setBuiltInLlmTemperature: (temp) => set({ builtInLlmTemperature: temp }),
      setOpenaiApiKey: (key) => set({ openaiApiKey: key }),
      setAnthropicApiKey: (key) => set({ anthropicApiKey: key }),
      setOllamaBaseUrl: (url) => set({ ollamaBaseUrl: url }),
      setSonaLearningEnabled: (enabled) => set({ sonaLearningEnabled: enabled }),
      setLearningRateSensitivity: (rate) => set({ learningRateSensitivity: rate }),
      setEncryptionAtRest: (enabled) => set({ encryptionAtRest: enabled }),
      setMasterPasswordHash: (hash) => set({ masterPasswordHash: hash }),
      setDataRetentionDays: (days) => set({ dataRetentionDays: days }),
      setTheme: (theme) => set({ theme }),
      setSidebarPosition: (position) => set({ sidebarPosition: position }),
      setEmailListDensity: (density) => set({ emailListDensity: density }),
      setFontSize: (size) => set({ fontSize: size }),
      resetAll: () => set(DEFAULT_STATE),
    }),
    {
      name: 'emailibrium-settings',
      version: 1,
      storage: createJSONStorage(() => localStorage),
      migrate: (persisted: unknown, version: number) => {
        const state = persisted as Record<string, unknown>;
        if (version === 0) {
          // Ensure llmProvider defaults to 'builtin' for users with older persisted state
          if (!state.llmProvider || state.llmProvider === 'none') {
            state.llmProvider = 'builtin';
          }
        }
        return state as unknown as SettingsState;
      },
      partialize: (state) => ({
        defaultComposeAccountId: state.defaultComposeAccountId,
        notificationsEnabled: state.notificationsEnabled,
        syncFrequencyMinutes: state.syncFrequencyMinutes,
        embeddingModel: state.embeddingModel,
        llmProvider: state.llmProvider,
        builtInLlmModel: state.builtInLlmModel,
        builtInLlmIdleTimeout: state.builtInLlmIdleTimeout,
        builtInLlmMaxContext: state.builtInLlmMaxContext,
        builtInLlmTemperature: state.builtInLlmTemperature,
        openaiApiKey: state.openaiApiKey,
        anthropicApiKey: state.anthropicApiKey,
        ollamaBaseUrl: state.ollamaBaseUrl,
        sonaLearningEnabled: state.sonaLearningEnabled,
        learningRateSensitivity: state.learningRateSensitivity,
        encryptionAtRest: state.encryptionAtRest,
        masterPasswordHash: state.masterPasswordHash,
        dataRetentionDays: state.dataRetentionDays,
        theme: state.theme,
        sidebarPosition: state.sidebarPosition,
        emailListDensity: state.emailListDensity,
        fontSize: state.fontSize,
      }),
    },
  ),
);

// Auto-sync settings to backend on every change (debounced).
useSettings.subscribe((state) => {
  syncToBackend(state as unknown as Record<string, unknown>);
});
