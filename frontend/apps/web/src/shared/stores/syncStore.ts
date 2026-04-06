import { create } from 'zustand';
import {
  getAccounts,
  startIngestion,
  getIngestionProgress,
  triggerReembed,
  PipelineBusyError,
} from '@emailibrium/api';

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

/** Human-readable label for each ingestion phase. */
const phaseLabels: Record<string, string> = {
  syncing: 'Fetching emails',
  embedding: 'Generating embeddings',
  categorizing: 'Categorizing emails',
  clustering: 'Building topic clusters',
  analyzing: 'Analyzing patterns',
  backfilling: 'AI categorization',
  complete: 'Complete',
};

/** Poll ingestion progress until the pipeline reaches "complete" or becomes inactive. */
async function waitForIngestionCompletion(
  emailAddress: string,
  onUpdate: (status: string) => void,
): Promise<void> {
  const maxWait = 240; // 240 polls × 3s = 12 min max (embedding can be slow)

  for (let i = 0; i < maxWait; i++) {
    await sleep(3000);
    try {
      const progress = await getIngestionProgress();

      if (!progress.active) {
        // Pipeline finished or was never started
        return;
      }

      const label = phaseLabels[progress.phase ?? ''] ?? progress.phase ?? 'Processing';
      const parts: string[] = [`${emailAddress}: ${label}`];

      if (progress.phase === 'syncing' && progress.processed) {
        parts.push(`(${progress.processed.toLocaleString()} emails)`);
      } else if (progress.phase === 'embedding' && progress.total) {
        const pct =
          progress.total > 0 ? Math.round(((progress.embedded ?? 0) / progress.total) * 100) : 0;
        parts.push(`(${pct}%)`);
      } else if (progress.phase === 'categorizing' && progress.total) {
        const pct =
          progress.total > 0 ? Math.round(((progress.categorized ?? 0) / progress.total) * 100) : 0;
        parts.push(`(${pct}%)`);
      }

      if (progress.etaSeconds && progress.etaSeconds > 0) {
        const mins = Math.ceil(progress.etaSeconds / 60);
        parts.push(`— ~${mins}m remaining`);
      }

      onUpdate(parts.join(' '));

      if (progress.phase === 'complete') {
        return;
      }
    } catch {
      // Ignore transient errors during polling
    }
  }
}

export const useSyncStore = create<SyncState>((set, get) => ({
  syncing: false,
  status: '',
  error: '',
  hasAccounts: false, // default to false until accounts are confirmed

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
          // Full sync: reembed already auto-triggered ingestion, so only
          // start ingestion explicitly for incremental syncs.
          if (mode !== 'full') {
            await startIngestion(a.id, 'manual_sync');
          }

          // Poll ingestion progress to show pipeline phases (embedding, clustering, etc.)
          await waitForIngestionCompletion(a.emailAddress, (status) => {
            set({ status });
          });
        } catch (err) {
          // Surface pipeline-busy conflicts with a specific message.
          if (err instanceof PipelineBusyError) {
            const { existingSource, existingPhase } = err.activity;
            const label =
              existingSource === 'inbox_clean'
                ? 'Inbox Clean'
                : existingSource === 'onboarding'
                  ? 'onboarding'
                  : existingSource === 'poll'
                    ? 'a background sync'
                    : 'another sync';
            set({
              syncing: false,
              status: '',
              error: `Cannot sync ${a.emailAddress}: ${label} is already running (${existingPhase} phase). Please wait for it to complete.`,
            });
            return;
          }
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
        // Clear status immediately — the pipeline banner in CommandCenter
        // now shows real-time progress from the ingestion endpoint, so we
        // don't need the Zustand "Sync complete!" message.
        set({ syncing: false, status: '', error: '' });
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      set({ syncing: false, status: '', error: `Sync failed: ${msg}` });
      console.error('Sync failed:', err);
    }
  },

  clearError: () => set({ error: '' }),
}));
