import { create } from 'zustand';
import type { EmailAccount, IngestionProgress } from '@emailibrium/types';

// --- Auth Store ---

export interface AuthState {
  accounts: EmailAccount[];
  currentAccount: EmailAccount | null;
  isAuthenticated: boolean;
  setAccounts: (accounts: EmailAccount[]) => void;
  addAccount: (account: EmailAccount) => void;
  removeAccount: (accountId: string) => void;
  setCurrentAccount: (account: EmailAccount | null) => void;
}

export const useAuthStore = create<AuthState>((set) => ({
  accounts: [],
  currentAccount: null,
  isAuthenticated: false,

  setAccounts: (accounts) =>
    set({
      accounts,
      isAuthenticated: accounts.length > 0,
    }),

  addAccount: (account) =>
    set((state) => {
      const accounts = [...state.accounts, account];
      return { accounts, isAuthenticated: true };
    }),

  removeAccount: (accountId) =>
    set((state) => {
      const accounts = state.accounts.filter((a) => a.id !== accountId);
      const currentAccount =
        state.currentAccount?.id === accountId ? null : state.currentAccount;
      return {
        accounts,
        currentAccount,
        isAuthenticated: accounts.length > 0,
      };
    }),

  setCurrentAccount: (account) => set({ currentAccount: account }),
}));

// --- UI Store ---

export type Theme = 'light' | 'dark' | 'system';

export interface UIState {
  sidebarOpen: boolean;
  theme: Theme;
  commandPaletteOpen: boolean;
  toggleSidebar: () => void;
  setTheme: (theme: Theme) => void;
  toggleCommandPalette: () => void;
}

export const useUIStore = create<UIState>((set) => ({
  sidebarOpen: true,
  theme: 'system',
  commandPaletteOpen: false,

  toggleSidebar: () => set((state) => ({ sidebarOpen: !state.sidebarOpen })),

  setTheme: (theme) => set({ theme }),

  toggleCommandPalette: () =>
    set((state) => ({ commandPaletteOpen: !state.commandPaletteOpen })),
}));

// --- Ingestion Store ---

export interface IngestionState {
  progress: IngestionProgress | null;
  isIngesting: boolean;
  setProgress: (progress: IngestionProgress) => void;
  clearProgress: () => void;
}

export const useIngestionStore = create<IngestionState>((set) => ({
  progress: null,
  isIngesting: false,

  setProgress: (progress) =>
    set({
      progress,
      isIngesting: progress.phase !== 'complete',
    }),

  clearProgress: () => set({ progress: null, isIngesting: false }),
}));
