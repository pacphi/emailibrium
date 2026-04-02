import { create } from 'zustand';
import { getAccounts, startIngestion, getEmails, triggerReembed } from '@emailibrium/api';

export type SyncMode = 'incremental' | 'full';

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
  /** Start syncing all active accounts. Mode: 'incremental' (default) or 'full' (reset + rebuild). */
  startSync: (mode?: SyncMode) => Promise<void>;
  /** Clear the error message. */
  clearError: () => void;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Poll until email count stabilizes (no new emails for 2 consecutive checks). */
async function waitForSyncCompletion(
  accountId: string,
  onUpdate: (count: number) => void,
): Promise<number> {
  let stableCount = 0;
  let stableChecks = 0;
  const maxWait = 120; // 120 polls × 3s = 6 min max

  for (let i = 0; i < maxWait; i++) {
    await sleep(3000);
    try {
      const { total } = await getEmails({ accountId, limit: 1, offset: 0 });
      onUpdate(total);

      if (total === stableCount) {
        stableChecks++;
        if (stableChecks >= 2) return total; // Stable for 6s — sync done
      } else {
        stableCount = total;
        stableChecks = 0;
      }
    } catch {
      // Ignore transient errors during polling
    }
  }
  return stableCount;
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

  startSync: async (mode: SyncMode = 'incremental') => {
    if (get().syncing) return; // prevent double-trigger

    set({
      syncing: true,
      status: mode === 'full' ? 'Rebuilding index...' : 'Fetching accounts...',
      error: '',
    });
    try {
      // Full sync: clear vectors, clusters, and reset all embedding statuses first.
      if (mode === 'full') {
        set({ status: 'Clearing vectors and clusters...' });
        await triggerReembed('all');
        set({ status: 'Fetching accounts...' });
      }

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

          // Poll until sync completes (email count stabilizes).
          await waitForSyncCompletion(a.id, (count) => {
            set({
              status: `Syncing ${a.emailAddress}... (${count.toLocaleString()} emails)`,
            });
          });
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
