// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// ---------------------------------------------------------------------------
// Mock the API module
// ---------------------------------------------------------------------------

const mockGetAccounts = vi.hoisted(() => vi.fn());
const mockStartIngestion = vi.hoisted(() => vi.fn());
const mockGetEmails = vi.hoisted(() => vi.fn());

vi.mock('@emailibrium/api', () => ({
  getAccounts: mockGetAccounts,
  startIngestion: mockStartIngestion,
  getEmails: mockGetEmails,
}));

// Must import AFTER vi.mock so the store picks up the mocked API.
import { useSyncStore } from '../syncStore';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Reset store state between tests. */
function resetStore() {
  useSyncStore.setState({
    syncing: false,
    status: '',
    error: '',
    hasAccounts: true,
  });
}

describe('syncStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    resetStore();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  // --- Initial state ---

  it('has correct initial state', () => {
    const state = useSyncStore.getState();
    expect(state.syncing).toBe(false);
    expect(state.status).toBe('');
    expect(state.error).toBe('');
    expect(state.hasAccounts).toBe(true);
  });

  // --- refreshAccounts ---

  it('refreshAccounts sets hasAccounts=true when active accounts exist', async () => {
    mockGetAccounts.mockResolvedValue([{ id: '1', emailAddress: 'a@b.com', isActive: true }]);

    await useSyncStore.getState().refreshAccounts();
    expect(useSyncStore.getState().hasAccounts).toBe(true);
  });

  it('refreshAccounts sets hasAccounts=false when no active accounts', async () => {
    mockGetAccounts.mockResolvedValue([{ id: '1', emailAddress: 'a@b.com', isActive: false }]);

    await useSyncStore.getState().refreshAccounts();
    expect(useSyncStore.getState().hasAccounts).toBe(false);
  });

  it('refreshAccounts sets hasAccounts=false on API error', async () => {
    mockGetAccounts.mockRejectedValue(new Error('Network error'));

    await useSyncStore.getState().refreshAccounts();
    expect(useSyncStore.getState().hasAccounts).toBe(false);
  });

  // --- clearError ---

  it('clearError clears the error message', () => {
    useSyncStore.setState({ error: 'Some error' });
    useSyncStore.getState().clearError();
    expect(useSyncStore.getState().error).toBe('');
  });

  // --- startSync ---

  it('startSync does nothing when already syncing', async () => {
    useSyncStore.setState({ syncing: true });

    await useSyncStore.getState().startSync();
    expect(mockGetAccounts).not.toHaveBeenCalled();
  });

  it('startSync sets hasAccounts=false when no active accounts', async () => {
    mockGetAccounts.mockResolvedValue([{ id: '1', emailAddress: 'a@b.com', isActive: false }]);

    await useSyncStore.getState().startSync();

    // The store redirects to /onboarding and resets syncing state.
    expect(useSyncStore.getState().syncing).toBe(false);
    expect(useSyncStore.getState().hasAccounts).toBe(false);
    expect(useSyncStore.getState().status).toBe('');
  });

  it('startSync sets error when getAccounts throws', async () => {
    mockGetAccounts.mockRejectedValue(new Error('Server down'));

    await useSyncStore.getState().startSync();

    expect(useSyncStore.getState().syncing).toBe(false);
    expect(useSyncStore.getState().error).toContain('Server down');
  });

  it('startSync calls startIngestion for each active account', async () => {
    mockGetAccounts.mockResolvedValue([
      { id: 'acc-1', emailAddress: 'a@b.com', isActive: true },
      { id: 'acc-2', emailAddress: 'c@d.com', isActive: true },
    ]);
    mockStartIngestion.mockResolvedValue({ jobId: 'job-1' });
    // Make getEmails stabilize immediately (same count twice)
    let callCount = 0;
    mockGetEmails.mockImplementation(async () => {
      callCount++;
      return { emails: [], total: 10 };
    });

    const syncPromise = useSyncStore.getState().startSync();

    // The sync loop uses sleep(3000) between polls.
    // We need to advance timers to let the polling complete.
    // Each account needs: at least 3 polls (initial + 2 stable checks) * 3000ms
    for (let i = 0; i < 10; i++) {
      await vi.advanceTimersByTimeAsync(3000);
    }

    await syncPromise;

    expect(mockStartIngestion).toHaveBeenCalledWith('acc-1');
    expect(mockStartIngestion).toHaveBeenCalledWith('acc-2');
    expect(mockStartIngestion).toHaveBeenCalledTimes(2);
  });

  it('startSync aggregates errors from multiple accounts', async () => {
    mockGetAccounts.mockResolvedValue([
      { id: 'acc-1', emailAddress: 'a@b.com', isActive: true },
      { id: 'acc-2', emailAddress: 'c@d.com', isActive: true },
    ]);
    mockStartIngestion.mockRejectedValue(new Error('Ingestion failed'));

    const syncPromise = useSyncStore.getState().startSync();
    await syncPromise;

    const state = useSyncStore.getState();
    expect(state.syncing).toBe(false);
    expect(state.error).toContain('Sync failed for 2 account(s)');
    expect(state.error).toContain('a@b.com');
  });

  it('startSync completes successfully and clears syncing flag', async () => {
    mockGetAccounts.mockResolvedValue([{ id: 'acc-1', emailAddress: 'a@b.com', isActive: true }]);
    mockStartIngestion.mockResolvedValue({ jobId: 'job-1' });
    mockGetEmails.mockResolvedValue({ emails: [], total: 5 });

    const syncPromise = useSyncStore.getState().startSync();

    // Advance timers just enough for polling stabilization (3 polls * 3s = 9s).
    // waitForSyncCompletion needs 2 consecutive stable checks.
    for (let i = 0; i < 4; i++) {
      await vi.advanceTimersByTimeAsync(3000);
    }

    await syncPromise;

    // Sync finished — syncing should be false, no errors.
    expect(useSyncStore.getState().syncing).toBe(false);
    expect(useSyncStore.getState().error).toBe('');
    // Status is either 'Sync complete!' or '' (if the 5s cleanup timer already fired).
    const status = useSyncStore.getState().status;
    expect(status === 'Sync complete!' || status === '').toBe(true);
  });
});
