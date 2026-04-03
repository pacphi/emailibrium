// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// ---------------------------------------------------------------------------
// Mock the API module
// ---------------------------------------------------------------------------

const { mockGetAccounts, mockStartIngestion, mockGetIngestionProgress, MockPipelineBusyError } =
  vi.hoisted(() => {
    class _MockPipelineBusyError extends Error {
      activity: Record<string, string>;
      constructor(body: Record<string, string>) {
        super(body.message);
        this.name = 'PipelineBusyError';
        this.activity = body;
      }
    }
    return {
      mockGetAccounts: vi.fn(),
      mockStartIngestion: vi.fn(),
      mockGetIngestionProgress: vi.fn(),
      MockPipelineBusyError: _MockPipelineBusyError,
    };
  });

vi.mock('@emailibrium/api', () => ({
  getAccounts: mockGetAccounts,
  startIngestion: mockStartIngestion,
  getIngestionProgress: mockGetIngestionProgress,
  triggerReembed: vi.fn().mockResolvedValue({ emailsReset: 0 }),
  PipelineBusyError: MockPipelineBusyError,
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
    // Simulate pipeline completing immediately
    mockGetIngestionProgress.mockResolvedValue({ active: false, phase: null });

    const syncPromise = useSyncStore.getState().startSync();

    // Advance past the first poll for each account
    for (let i = 0; i < 4; i++) {
      await vi.advanceTimersByTimeAsync(3000);
    }

    await syncPromise;

    expect(mockStartIngestion).toHaveBeenCalledWith('acc-1', 'manual_sync');
    expect(mockStartIngestion).toHaveBeenCalledWith('acc-2', 'manual_sync');
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

  it('startSync surfaces PipelineBusyError with specific message', async () => {
    mockGetAccounts.mockResolvedValue([{ id: 'acc-1', emailAddress: 'a@b.com', isActive: true }]);
    mockStartIngestion.mockRejectedValue(
      new MockPipelineBusyError({
        error: 'pipeline_busy',
        message: 'A poll operation is already in progress',
        existingJobId: 'job-123',
        existingSource: 'poll',
        existingPhase: 'embedding',
        startedAt: '2026-04-03T10:00:00Z',
      }),
    );

    await useSyncStore.getState().startSync();

    const state = useSyncStore.getState();
    expect(state.syncing).toBe(false);
    expect(state.error).toContain('a background sync');
    expect(state.error).toContain('embedding phase');
  });

  it('startSync shows phase progress and completes successfully', async () => {
    mockGetAccounts.mockResolvedValue([{ id: 'acc-1', emailAddress: 'a@b.com', isActive: true }]);
    mockStartIngestion.mockResolvedValue({ jobId: 'job-1' });

    // Simulate phases: syncing → embedding → complete
    let callCount = 0;
    mockGetIngestionProgress.mockImplementation(async () => {
      callCount++;
      if (callCount === 1) {
        return { active: true, phase: 'syncing', total: 100, processed: 50 };
      }
      if (callCount === 2) {
        return { active: true, phase: 'embedding', total: 100, embedded: 60 };
      }
      return { active: true, phase: 'complete' };
    });

    const syncPromise = useSyncStore.getState().startSync();

    // Advance timers for 3 polls
    for (let i = 0; i < 4; i++) {
      await vi.advanceTimersByTimeAsync(3000);
    }

    await syncPromise;

    // Sync finished — syncing should be false, no errors.
    expect(useSyncStore.getState().syncing).toBe(false);
    expect(useSyncStore.getState().error).toBe('');
    const status = useSyncStore.getState().status;
    expect(status === 'Sync complete!' || status === '').toBe(true);
  });
});
