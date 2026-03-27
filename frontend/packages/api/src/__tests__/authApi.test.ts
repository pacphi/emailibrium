import { describe, it, expect, vi, beforeEach } from 'vitest';

// ---------------------------------------------------------------------------
// Mocks (vitest 4: hoist mock variables)
// ---------------------------------------------------------------------------

const { mockGet, mockPost, mockDelete, mockPatch, jsonFn } = vi.hoisted(() => {
  const jsonFn = vi.fn().mockResolvedValue({});
  const responseLike = () => ({ json: jsonFn });

  return {
    mockGet: vi.fn().mockImplementation(() => responseLike()),
    mockPost: vi.fn().mockImplementation(() => responseLike()),
    mockDelete: vi.fn().mockImplementation(() => responseLike()),
    mockPatch: vi.fn().mockImplementation(() => responseLike()),
    jsonFn,
  };
});

vi.mock('../client.js', () => ({
  api: {
    get: mockGet,
    post: mockPost,
    delete: mockDelete,
    patch: mockPatch,
  },
}));

import {
  connectGmail,
  connectOutlook,
  connectImap,
  getAccounts,
  disconnectAccount,
  updateAccount,
  removeAccountLabels,
  unarchiveAccount,
} from '../authApi.js';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('authApi', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    jsonFn.mockResolvedValue({});
  });

  describe('connectGmail', () => {
    it('calls POST auth/gmail/connect and returns authUrl', async () => {
      jsonFn.mockResolvedValueOnce({ authUrl: 'https://accounts.google.com/...' });

      const result = await connectGmail();

      expect(mockPost).toHaveBeenCalledWith('auth/gmail/connect');
      expect(result).toEqual({ authUrl: 'https://accounts.google.com/...' });
    });
  });

  describe('connectOutlook', () => {
    it('calls POST auth/outlook/connect', async () => {
      jsonFn.mockResolvedValueOnce({ authUrl: 'https://login.microsoftonline.com/...' });

      const result = await connectOutlook();

      expect(mockPost).toHaveBeenCalledWith('auth/outlook/connect');
      expect(result.authUrl).toBeDefined();
    });
  });

  describe('connectImap', () => {
    it('calls POST auth/imap/connect with IMAP config JSON', async () => {
      const config = {
        host: 'imap.example.com',
        port: 993,
        username: 'user',
        password: 'pass',
        tls: true,
      };
      const account = { id: 'acc1', provider: 'imap' };
      jsonFn.mockResolvedValueOnce(account);

      const result = await connectImap(config as never);

      expect(mockPost).toHaveBeenCalledWith('auth/imap/connect', { json: config });
      expect(result).toEqual(account);
    });
  });

  describe('getAccounts', () => {
    it('calls GET auth/accounts', async () => {
      const accounts = [{ id: 'acc1' }, { id: 'acc2' }];
      jsonFn.mockResolvedValueOnce(accounts);

      const result = await getAccounts();

      expect(mockGet).toHaveBeenCalledWith('auth/accounts');
      expect(result).toEqual(accounts);
    });
  });

  describe('disconnectAccount', () => {
    it('calls DELETE auth/accounts/:id', async () => {
      await disconnectAccount('acc1');
      expect(mockDelete).toHaveBeenCalledWith('auth/accounts/acc1');
    });
  });

  describe('updateAccount', () => {
    it('calls PATCH auth/accounts/:id with changes JSON', async () => {
      const changes = { displayName: 'Work Email' };
      await updateAccount('acc1', changes);
      expect(mockPatch).toHaveBeenCalledWith('auth/accounts/acc1', { json: changes });
    });
  });

  describe('removeAccountLabels', () => {
    it('calls POST auth/accounts/:id/remove-labels', async () => {
      jsonFn.mockResolvedValueOnce({ messagesProcessed: 5, labelsDeleted: 3 });

      const result = await removeAccountLabels('acc1');

      expect(mockPost).toHaveBeenCalledWith('auth/accounts/acc1/remove-labels');
      expect(result).toEqual({ messagesProcessed: 5, labelsDeleted: 3 });
    });
  });

  describe('unarchiveAccount', () => {
    it('calls POST auth/accounts/:id/unarchive', async () => {
      jsonFn.mockResolvedValueOnce({ messagesProcessed: 10 });

      const result = await unarchiveAccount('acc1');

      expect(mockPost).toHaveBeenCalledWith('auth/accounts/acc1/unarchive');
      expect(result).toEqual({ messagesProcessed: 10 });
    });
  });
});
