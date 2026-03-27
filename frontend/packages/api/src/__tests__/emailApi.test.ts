import { describe, it, expect, vi, beforeEach } from 'vitest';

// ---------------------------------------------------------------------------
// Mocks (vitest 4: hoist mock variables)
// ---------------------------------------------------------------------------

const { mockGet, mockPost, mockDelete, jsonFn } = vi.hoisted(() => {
  const jsonFn = vi.fn().mockResolvedValue({});
  const responseLike = () => ({ json: jsonFn });

  return {
    mockGet: vi.fn().mockImplementation(() => responseLike()),
    mockPost: vi.fn().mockImplementation(() => responseLike()),
    mockDelete: vi.fn().mockImplementation(() => responseLike()),
    jsonFn,
  };
});

vi.mock('../client.js', () => ({
  api: {
    get: mockGet,
    post: mockPost,
    delete: mockDelete,
  },
}));

import {
  getEmails,
  getEmail,
  getThread,
  archiveEmail,
  starEmail,
  markEmailRead,
  deleteEmail,
  sendEmail,
  replyToEmail,
  forwardEmail,
  getCategories,
  getLabels,
  moveEmail,
  getAttachments,
  getAttachmentDownloadUrl,
  getAttachmentsZipUrl,
} from '../emailApi.js';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('emailApi', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    jsonFn.mockResolvedValue({});
  });

  // -----------------------------------------------------------------------
  // getEmails
  // -----------------------------------------------------------------------

  describe('getEmails', () => {
    it('calls GET emails with search params', async () => {
      const data = { emails: [], total: 0 };
      jsonFn.mockResolvedValueOnce(data);

      const result = await getEmails({ accountId: 'acc1', limit: 10, offset: 0 });

      expect(mockGet).toHaveBeenCalledWith('emails', {
        searchParams: { accountId: 'acc1', limit: 10, offset: 0 },
      });
      expect(result).toEqual(data);
    });

    it('calls GET emails without params when none provided', async () => {
      jsonFn.mockResolvedValueOnce({ emails: [], total: 0 });

      await getEmails();

      expect(mockGet).toHaveBeenCalledWith('emails', {
        searchParams: undefined,
      });
    });
  });

  // -----------------------------------------------------------------------
  // getEmail
  // -----------------------------------------------------------------------

  describe('getEmail', () => {
    it('calls GET emails/:id', async () => {
      const email = { id: 'e1', subject: 'Hello' };
      jsonFn.mockResolvedValueOnce(email);

      const result = await getEmail('e1');

      expect(mockGet).toHaveBeenCalledWith('emails/e1');
      expect(result).toEqual(email);
    });
  });

  // -----------------------------------------------------------------------
  // getThread
  // -----------------------------------------------------------------------

  describe('getThread', () => {
    it('calls GET emails/thread/:threadId', async () => {
      const thread = { id: 'th1', emails: [] };
      jsonFn.mockResolvedValueOnce(thread);

      const result = await getThread('th1');

      expect(mockGet).toHaveBeenCalledWith('emails/thread/th1');
      expect(result).toEqual(thread);
    });
  });

  // -----------------------------------------------------------------------
  // archiveEmail
  // -----------------------------------------------------------------------

  describe('archiveEmail', () => {
    it('calls POST emails/:id/archive', async () => {
      await archiveEmail('e1');
      expect(mockPost).toHaveBeenCalledWith('emails/e1/archive');
    });
  });

  // -----------------------------------------------------------------------
  // starEmail
  // -----------------------------------------------------------------------

  describe('starEmail', () => {
    it('calls POST emails/:id/star', async () => {
      await starEmail('e1');
      expect(mockPost).toHaveBeenCalledWith('emails/e1/star');
    });
  });

  // -----------------------------------------------------------------------
  // markEmailRead
  // -----------------------------------------------------------------------

  describe('markEmailRead', () => {
    it('calls POST emails/:id/read with json body', async () => {
      await markEmailRead('e1', true);
      expect(mockPost).toHaveBeenCalledWith('emails/e1/read', { json: { read: true } });
    });

    it('passes false in body for marking unread', async () => {
      await markEmailRead('e1', false);
      expect(mockPost).toHaveBeenCalledWith('emails/e1/read', { json: { read: false } });
    });
  });

  // -----------------------------------------------------------------------
  // deleteEmail
  // -----------------------------------------------------------------------

  describe('deleteEmail', () => {
    it('calls DELETE emails/:id', async () => {
      await deleteEmail('e1');
      expect(mockDelete).toHaveBeenCalledWith('emails/e1');
    });
  });

  // -----------------------------------------------------------------------
  // sendEmail
  // -----------------------------------------------------------------------

  describe('sendEmail', () => {
    it('calls POST emails/send with draft JSON', async () => {
      const draft = {
        to: 'user@example.com',
        subject: 'Test',
        bodyText: 'Hello',
        accountId: 'acc1',
      };
      jsonFn.mockResolvedValueOnce({ messageId: 'msg1' });

      const result = await sendEmail(draft);

      expect(mockPost).toHaveBeenCalledWith('emails/send', { json: draft });
      expect(result).toEqual({ messageId: 'msg1' });
    });
  });

  // -----------------------------------------------------------------------
  // replyToEmail
  // -----------------------------------------------------------------------

  describe('replyToEmail', () => {
    it('calls POST emails/:id/reply with body', async () => {
      const body = { bodyText: 'Thanks!' };
      jsonFn.mockResolvedValueOnce({ messageId: 'msg2' });

      const result = await replyToEmail('e1', body);

      expect(mockPost).toHaveBeenCalledWith('emails/e1/reply', { json: body });
      expect(result).toEqual({ messageId: 'msg2' });
    });
  });

  // -----------------------------------------------------------------------
  // forwardEmail
  // -----------------------------------------------------------------------

  describe('forwardEmail', () => {
    it('calls POST emails/:id/forward with to address', async () => {
      jsonFn.mockResolvedValueOnce({ messageId: 'msg3' });

      const result = await forwardEmail('e1', 'other@example.com');

      expect(mockPost).toHaveBeenCalledWith('emails/e1/forward', {
        json: { to: 'other@example.com' },
      });
      expect(result).toEqual({ messageId: 'msg3' });
    });
  });

  // -----------------------------------------------------------------------
  // getCategories / getLabels
  // -----------------------------------------------------------------------

  describe('getCategories', () => {
    it('calls GET emails/categories', async () => {
      jsonFn.mockResolvedValueOnce({ categories: ['Inbox', 'Promotions'] });

      const result = await getCategories();

      expect(mockGet).toHaveBeenCalledWith('emails/categories');
      expect(result).toEqual({ categories: ['Inbox', 'Promotions'] });
    });
  });

  describe('getLabels', () => {
    it('calls GET emails/labels with accountId param', async () => {
      jsonFn.mockResolvedValueOnce([]);

      await getLabels('acc1');

      expect(mockGet).toHaveBeenCalledWith('emails/labels', {
        searchParams: { accountId: 'acc1' },
      });
    });
  });

  // -----------------------------------------------------------------------
  // moveEmail
  // -----------------------------------------------------------------------

  describe('moveEmail', () => {
    it('calls POST emails/:id/move with body', async () => {
      const body = { accountId: 'acc1', targetId: 'folder1', kind: 'folder' as const };
      await moveEmail('e1', body);
      expect(mockPost).toHaveBeenCalledWith('emails/e1/move', { json: body });
    });
  });

  // -----------------------------------------------------------------------
  // getAttachments
  // -----------------------------------------------------------------------

  describe('getAttachments', () => {
    it('calls GET emails/:emailId/attachments', async () => {
      jsonFn.mockResolvedValueOnce([]);

      await getAttachments('e1');

      expect(mockGet).toHaveBeenCalledWith('emails/e1/attachments');
    });
  });

  // -----------------------------------------------------------------------
  // URL helpers (synchronous, no API call)
  // -----------------------------------------------------------------------

  describe('URL helpers', () => {
    it('getAttachmentDownloadUrl returns correct URL', () => {
      const url = getAttachmentDownloadUrl('e1', 'att1');
      expect(url).toBe('/api/v1/emails/e1/attachments/att1');
    });

    it('getAttachmentsZipUrl returns correct URL', () => {
      const url = getAttachmentsZipUrl('e1');
      expect(url).toBe('/api/v1/emails/e1/attachments/zip');
    });
  });
});
