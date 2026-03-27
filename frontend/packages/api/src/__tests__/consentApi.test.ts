import { describe, it, expect, vi, beforeEach } from 'vitest';

// ---------------------------------------------------------------------------
// Mocks (vitest 4: hoist mock variables)
// ---------------------------------------------------------------------------

const { mockGet, mockPost, jsonFn } = vi.hoisted(() => {
  const jsonFn = vi.fn().mockResolvedValue({});
  const responseLike = () => ({ json: jsonFn });

  return {
    mockGet: vi.fn().mockImplementation(() => responseLike()),
    mockPost: vi.fn().mockImplementation(() => responseLike()),
    jsonFn,
  };
});

vi.mock('../client.js', () => ({
  api: {
    get: mockGet,
    post: mockPost,
  },
}));

import {
  recordConsent,
  getConsents,
  requestDataExport,
  requestDataErase,
} from '../consentApi.js';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('consentApi', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    jsonFn.mockResolvedValue({});
  });

  describe('recordConsent', () => {
    it('calls POST consent/gdpr with consent record JSON', async () => {
      const consent = { purpose: 'analytics', granted: true } as never;
      const response = { id: 'c1', purpose: 'analytics', granted: true };
      jsonFn.mockResolvedValueOnce(response);

      const result = await recordConsent(consent);

      expect(mockPost).toHaveBeenCalledWith('consent/gdpr', { json: consent });
      expect(result).toEqual(response);
    });
  });

  describe('getConsents', () => {
    it('calls GET consent/gdpr and returns decisions array', async () => {
      const decisions = [{ id: 'c1', purpose: 'analytics' }];
      jsonFn.mockResolvedValueOnce({ decisions });

      const result = await getConsents();

      expect(mockGet).toHaveBeenCalledWith('consent/gdpr');
      expect(result).toEqual(decisions);
    });

    it('returns empty array when decisions is undefined', async () => {
      jsonFn.mockResolvedValueOnce({});

      const result = await getConsents();

      expect(result).toEqual([]);
    });
  });

  describe('requestDataExport', () => {
    it('calls POST consent/export with request JSON', async () => {
      const request = { format: 'json' } as never;
      const response = { exportId: 'exp1', status: 'pending' };
      jsonFn.mockResolvedValueOnce(response);

      const result = await requestDataExport(request);

      expect(mockPost).toHaveBeenCalledWith('consent/export', { json: request });
      expect(result).toEqual(response);
    });
  });

  describe('requestDataErase', () => {
    it('calls POST consent/erase with confirmation JSON', async () => {
      const response = { status: 'completed' };
      jsonFn.mockResolvedValueOnce(response);

      const result = await requestDataErase({ confirm: true });

      expect(mockPost).toHaveBeenCalledWith('consent/erase', {
        json: { confirm: true },
      });
      expect(result).toEqual(response);
    });
  });
});
