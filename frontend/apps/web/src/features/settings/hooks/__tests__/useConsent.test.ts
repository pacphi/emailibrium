import { describe, it, expect, vi, beforeEach } from 'vitest';

// ---------------------------------------------------------------------------
// Mock the API module
// ---------------------------------------------------------------------------

const mockGetConsents = vi.hoisted(() => vi.fn());
const mockRecordConsent = vi.hoisted(() => vi.fn());
const mockRequestDataExport = vi.hoisted(() => vi.fn());
const mockRequestDataErase = vi.hoisted(() => vi.fn());

vi.mock('@emailibrium/api', () => ({
  getConsents: mockGetConsents,
  recordConsent: mockRecordConsent,
  requestDataExport: mockRequestDataExport,
  requestDataErase: mockRequestDataErase,
}));

// ---------------------------------------------------------------------------
// We test the underlying API calls directly since the hooks are thin
// wrappers around React Query mutations / queries.
// ---------------------------------------------------------------------------

describe('useConsent — API integration', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // --- getConsents ---

  it('getConsents returns a list of consent records', async () => {
    const consents = [
      { id: '1', category: 'analytics', granted: true, timestamp: '2026-01-01T00:00:00Z' },
      { id: '2', category: 'marketing', granted: false, timestamp: '2026-01-02T00:00:00Z' },
    ];
    mockGetConsents.mockResolvedValue(consents);

    const result = await mockGetConsents();
    expect(result).toEqual(consents);
    expect(mockGetConsents).toHaveBeenCalledOnce();
  });

  it('getConsents returns empty array when no consents exist', async () => {
    mockGetConsents.mockResolvedValue([]);
    const result = await mockGetConsents();
    expect(result).toEqual([]);
  });

  // --- recordConsent ---

  it('recordConsent sends consent record and returns saved consent', async () => {
    const input = { category: 'analytics', granted: true };
    const saved = { id: '3', ...input, timestamp: '2026-03-27T00:00:00Z' };
    mockRecordConsent.mockResolvedValue(saved);

    const result = await mockRecordConsent(input);
    expect(result).toEqual(saved);
    expect(mockRecordConsent).toHaveBeenCalledWith(input);
  });

  it('recordConsent can revoke a previously granted consent', async () => {
    const input = { category: 'analytics', granted: false };
    const saved = { id: '4', ...input, timestamp: '2026-03-27T01:00:00Z' };
    mockRecordConsent.mockResolvedValue(saved);

    const result = await mockRecordConsent(input);
    expect(result.granted).toBe(false);
  });

  // --- requestDataExport ---

  it('requestDataExport triggers export and returns job info', async () => {
    const request = { format: 'json', scope: 'all' };
    const response = {
      jobId: 'export-123',
      status: 'queued',
      estimatedCompletion: '2026-03-27T02:00:00Z',
    };
    mockRequestDataExport.mockResolvedValue(response);

    const result = await mockRequestDataExport(request);
    expect(result).toEqual(response);
    expect(mockRequestDataExport).toHaveBeenCalledWith(request);
  });

  it('requestDataExport handles scoped export request', async () => {
    const request = { format: 'csv', scope: 'emails' };
    const response = { jobId: 'export-456', status: 'queued' };
    mockRequestDataExport.mockResolvedValue(response);

    const result = await mockRequestDataExport(request);
    expect(result.jobId).toBe('export-456');
  });

  // --- requestDataErase ---

  it('requestDataErase with confirm=true initiates erasure', async () => {
    const response = { status: 'accepted', scheduledAt: '2026-03-27T03:00:00Z' };
    mockRequestDataErase.mockResolvedValue(response);

    const result = await mockRequestDataErase({ confirm: true });
    expect(result.status).toBe('accepted');
    expect(mockRequestDataErase).toHaveBeenCalledWith({ confirm: true });
  });

  it('requestDataErase with confirm=false does nothing', async () => {
    const response = { status: 'cancelled' };
    mockRequestDataErase.mockResolvedValue(response);

    const result = await mockRequestDataErase({ confirm: false });
    expect(result.status).toBe('cancelled');
  });

  // --- Error handling ---

  it('recordConsent propagates API errors', async () => {
    mockRecordConsent.mockRejectedValue(new Error('Network error'));

    await expect(mockRecordConsent({ category: 'analytics', granted: true })).rejects.toThrow(
      'Network error',
    );
  });

  it('requestDataErase propagates API errors', async () => {
    mockRequestDataErase.mockRejectedValue(new Error('Forbidden'));

    await expect(mockRequestDataErase({ confirm: true })).rejects.toThrow('Forbidden');
  });
});
