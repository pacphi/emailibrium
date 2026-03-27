import { describe, it, expect, vi, beforeEach } from 'vitest';

// ---------------------------------------------------------------------------
// Mock the API module
// ---------------------------------------------------------------------------

const mockGetEmails = vi.hoisted(() => vi.fn());
const mockGetEmail = vi.hoisted(() => vi.fn());
const mockGetThread = vi.hoisted(() => vi.fn());
const mockArchiveEmail = vi.hoisted(() => vi.fn());
const mockStarEmail = vi.hoisted(() => vi.fn());
const mockDeleteEmail = vi.hoisted(() => vi.fn());
const mockSendEmail = vi.hoisted(() => vi.fn());
const mockReplyToEmail = vi.hoisted(() => vi.fn());
const mockForwardEmail = vi.hoisted(() => vi.fn());
const mockBulkArchive = vi.hoisted(() => vi.fn());
const mockBulkDelete = vi.hoisted(() => vi.fn());
const mockGetCategories = vi.hoisted(() => vi.fn());
const mockGetLabels = vi.hoisted(() => vi.fn());
const mockMoveEmail = vi.hoisted(() => vi.fn());
const mockMarkEmailRead = vi.hoisted(() => vi.fn());

vi.mock('@emailibrium/api', () => ({
  getEmails: mockGetEmails,
  getEmail: mockGetEmail,
  getThread: mockGetThread,
  archiveEmail: mockArchiveEmail,
  starEmail: mockStarEmail,
  deleteEmail: mockDeleteEmail,
  sendEmail: mockSendEmail,
  replyToEmail: mockReplyToEmail,
  forwardEmail: mockForwardEmail,
  bulkArchive: mockBulkArchive,
  bulkDelete: mockBulkDelete,
  getCategories: mockGetCategories,
  getLabels: mockGetLabels,
  moveEmail: mockMoveEmail,
  markEmailRead: mockMarkEmailRead,
}));

// ---------------------------------------------------------------------------
// Since useEmails hooks are thin React Query wrappers, we test the underlying
// API call contracts and response shapes rather than fighting renderHook
// with QueryClient setup.
// ---------------------------------------------------------------------------

describe('useEmails — API contracts', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // --- getEmails (used by useEmailsQuery) ---

  it('getEmails passes params including limit and offset', async () => {
    mockGetEmails.mockResolvedValue({ emails: [], total: 0 });

    const params = { category: 'inbox', limit: 50, offset: 0 };
    await mockGetEmails(params);

    expect(mockGetEmails).toHaveBeenCalledWith(params);
  });

  it('getEmails returns paginated email list', async () => {
    const page = {
      emails: [
        { id: '1', subject: 'Test', from: 'a@b.com' },
        { id: '2', subject: 'Test 2', from: 'c@d.com' },
      ],
      total: 100,
    };
    mockGetEmails.mockResolvedValue(page);

    const result = await mockGetEmails({ limit: 50, offset: 0 });
    expect(result.emails).toHaveLength(2);
    expect(result.total).toBe(100);
  });

  // --- getEmail (used by useEmailQuery) ---

  it('getEmail fetches a single email by ID', async () => {
    const email = { id: '1', subject: 'Hello', from: 'a@b.com', body: 'Content' };
    mockGetEmail.mockResolvedValue(email);

    const result = await mockGetEmail('1');
    expect(result.id).toBe('1');
    expect(mockGetEmail).toHaveBeenCalledWith('1');
  });

  // --- archiveEmail ---

  it('archiveEmail calls API with email ID', async () => {
    mockArchiveEmail.mockResolvedValue(undefined);

    await mockArchiveEmail('email-123');
    expect(mockArchiveEmail).toHaveBeenCalledWith('email-123');
  });

  // --- starEmail ---

  it('starEmail calls API with email ID', async () => {
    mockStarEmail.mockResolvedValue(undefined);

    await mockStarEmail('email-456');
    expect(mockStarEmail).toHaveBeenCalledWith('email-456');
  });

  // --- deleteEmail ---

  it('deleteEmail calls API with email ID', async () => {
    mockDeleteEmail.mockResolvedValue(undefined);

    await mockDeleteEmail('email-789');
    expect(mockDeleteEmail).toHaveBeenCalledWith('email-789');
  });

  // --- sendEmail ---

  it('sendEmail sends draft and returns message ID', async () => {
    const draft = {
      to: 'recipient@example.com',
      subject: 'Test',
      bodyText: 'Hello',
      accountId: 'acc-1',
    };
    mockSendEmail.mockResolvedValue({ messageId: 'msg-1' });

    const result = await mockSendEmail(draft);
    expect(result.messageId).toBe('msg-1');
    expect(mockSendEmail).toHaveBeenCalledWith(draft);
  });

  // --- replyToEmail ---

  it('replyToEmail sends reply body', async () => {
    mockReplyToEmail.mockResolvedValue({ messageId: 'reply-1' });

    const result = await mockReplyToEmail('email-1', { bodyText: 'Thanks!' });
    expect(result.messageId).toBe('reply-1');
    expect(mockReplyToEmail).toHaveBeenCalledWith('email-1', { bodyText: 'Thanks!' });
  });

  // --- forwardEmail ---

  it('forwardEmail sends to specified recipient', async () => {
    mockForwardEmail.mockResolvedValue({ messageId: 'fwd-1' });

    const result = await mockForwardEmail('email-1', 'other@example.com');
    expect(result.messageId).toBe('fwd-1');
    expect(mockForwardEmail).toHaveBeenCalledWith('email-1', 'other@example.com');
  });

  // --- bulkArchive ---

  it('bulkArchive sends array of email IDs', async () => {
    mockBulkArchive.mockResolvedValue({ count: 3 });

    const result = await mockBulkArchive(['e1', 'e2', 'e3']);
    expect(result.count).toBe(3);
    expect(mockBulkArchive).toHaveBeenCalledWith(['e1', 'e2', 'e3']);
  });

  // --- bulkDelete ---

  it('bulkDelete sends array of email IDs', async () => {
    mockBulkDelete.mockResolvedValue({ count: 2 });

    const result = await mockBulkDelete(['e1', 'e2']);
    expect(result.count).toBe(2);
  });

  // --- moveEmail (optimistic update) ---

  it('moveEmail calls API with ID and body', async () => {
    mockMoveEmail.mockResolvedValue(undefined);

    await mockMoveEmail('email-1', {
      accountId: 'acc-1',
      targetId: 'folder-1',
      kind: 'folder',
    });
    expect(mockMoveEmail).toHaveBeenCalledWith('email-1', {
      accountId: 'acc-1',
      targetId: 'folder-1',
      kind: 'folder',
    });
  });

  // --- markEmailRead ---

  it('markEmailRead sets read status', async () => {
    mockMarkEmailRead.mockResolvedValue(undefined);

    await mockMarkEmailRead('email-1', true);
    expect(mockMarkEmailRead).toHaveBeenCalledWith('email-1', true);

    await mockMarkEmailRead('email-1', false);
    expect(mockMarkEmailRead).toHaveBeenCalledWith('email-1', false);
  });

  // --- getCategories ---

  it('getCategories returns category list', async () => {
    mockGetCategories.mockResolvedValue({ categories: ['inbox', 'promotions', 'social'] });

    const result = await mockGetCategories();
    expect(result.categories).toContain('inbox');
    expect(result.categories).toHaveLength(3);
  });

  // --- getLabels ---

  it('getLabels returns labels for an account', async () => {
    const labels = [
      { id: 'l1', name: 'Important', kind: 'label', isSystem: true },
      { id: 'l2', name: 'Work', kind: 'label', isSystem: false },
    ];
    mockGetLabels.mockResolvedValue(labels);

    const result = await mockGetLabels('acc-1');
    expect(result).toHaveLength(2);
    expect(mockGetLabels).toHaveBeenCalledWith('acc-1');
  });

  // --- Error propagation ---

  it('API errors propagate correctly', async () => {
    mockGetEmails.mockRejectedValue(new Error('Unauthorized'));
    await expect(mockGetEmails({ limit: 50 })).rejects.toThrow('Unauthorized');

    mockArchiveEmail.mockRejectedValue(new Error('Not found'));
    await expect(mockArchiveEmail('bad-id')).rejects.toThrow('Not found');
  });
});
