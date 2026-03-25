import { create } from 'zustand';
import { getAccounts, startIngestion } from '@emailibrium/api';

export interface SyncState {
  /** Whether a sync is currently in progress. */
  syncing: boolean;
  /** Human-readable status message (e.g., "Syncing account 1 of 2: ..."). */
  status: string;
  /** Error message from the last sync attempt, if any. */
  error: string;
  /** Whether at least one active account exists. */
  hasAccounts: boolean;

  /** Check for active accounts (call on mount). */
  refreshAccounts: () => Promise<void>;
  /** Start syncing all active accounts sequentially. */
  startSync: () => Promise<void>;
  /** Clear the error message. */
  clearError: () => void;
}

export const useSyncStore = create<SyncState>((set, get) => ({
  syncing: false,
  status: '',
  error: '',
  hasAccounts: true, // optimistic default

  refreshAccounts: async () => {
    try {
      const accounts = await getAccounts();
      set({ hasAccounts: accounts.some((a) => a.isActive) });
    } catch {
      set({ hasAccounts: false });
    }
  },

  startSync: async () => {
    if (get().syncing) return; // prevent double-trigger

    set({ syncing: true, status: 'Fetching accounts...', error: '' });
    try {
      const accounts = await getAccounts();
      const active = accounts.filter((a) => a.isActive);

      if (active.length === 0) {
        set({ syncing: false, status: '', hasAccounts: false });
        window.location.href = '/onboarding';
        return;
      }

      set({ hasAccounts: true });
      const errors: string[] = [];

      for (let i = 0; i < active.length; i++) {
        const a = active[i]!;
        set({
          status: `Syncing account ${i + 1} of ${active.length}: ${a.emailAddress}...`,
        });
        try {
          await startIngestion(a.id);
        } catch (err) {
          const msg = err instanceof Error ? err.message : String(err);
          errors.push(`${a.emailAddress}: ${msg}`);
          console.error(`Sync failed for ${a.emailAddress}:`, err);
        }
      }

      if (errors.length > 0) {
        set({
          syncing: false,
          status: '',
          error: `Sync failed for ${errors.length} account(s): ${errors[0]}`,
        });
      } else {
        set({ syncing: false, status: 'Sync complete!', error: '' });
        setTimeout(() => {
          // Only clear if still showing "complete" (not re-triggered).
          if (get().status === 'Sync complete!') {
            set({ status: '' });
          }
        }, 5000);
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      set({ syncing: false, status: '', error: `Sync failed: ${msg}` });
      console.error('Sync failed:', err);
    }
  },

  clearError: () => set({ error: '' }),
}));
