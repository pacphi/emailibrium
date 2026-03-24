import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
export type Theme = 'light' | 'dark' | 'system';
export type SidebarPosition = 'left' | 'right';
export type EmailListDensity = 'compact' | 'comfortable' | 'spacious';
export type LlmProvider = 'local' | 'openai' | 'anthropic';

export interface SettingsState {
  // General
  defaultComposeAccountId: string | null;
  notificationsEnabled: boolean;
  syncFrequencyMinutes: number;

  // AI / LLM
  embeddingModel: string;
  llmProvider: LlmProvider;
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
  embeddingModel: 'text-embedding-3-small',
  llmProvider: 'local' as LlmProvider,
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
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({
        defaultComposeAccountId: state.defaultComposeAccountId,
        notificationsEnabled: state.notificationsEnabled,
        syncFrequencyMinutes: state.syncFrequencyMinutes,
        embeddingModel: state.embeddingModel,
        llmProvider: state.llmProvider,
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
