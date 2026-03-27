import { describe, it, expect, vi, beforeEach } from 'vitest';

// ---------------------------------------------------------------------------
// Mocks (vitest 4: hoist mock variables)
// ---------------------------------------------------------------------------

const { mockGet, mockSet, mockDel } = vi.hoisted(() => ({
  mockGet: vi.fn(),
  mockSet: vi.fn().mockResolvedValue(undefined),
  mockDel: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('idb-keyval', () => ({
  get: mockGet,
  set: mockSet,
  del: mockDel,
}));

import { secureStorage } from '../secureStorage';

// ---------------------------------------------------------------------------
// Helpers — real Web Crypto polyfill for Node/test environments
// ---------------------------------------------------------------------------

/**
 * We rely on the real `crypto.subtle` API available in Node >= 15.
 * If it's missing (very old Node), these tests will naturally be skipped
 * because every assertion will throw.
 */

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('secureStorage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Each test starts with no cached key in IndexedDB
    mockGet.mockReset();
    mockSet.mockReset().mockResolvedValue(undefined);
    mockDel.mockReset().mockResolvedValue(undefined);
  });

  // -----------------------------------------------------------------------
  // Key generation & caching
  // -----------------------------------------------------------------------

  describe('key generation and caching', () => {
    it('generates a new CryptoKey when none exists in IndexedDB', async () => {
      // First call to get (for the key handle) returns undefined
      mockGet.mockResolvedValueOnce(undefined);

      await secureStorage.setItem('foo', 'bar');

      // A key should have been stored via set()
      expect(mockSet).toHaveBeenCalledWith(
        '__emailibrium_crypto_key__',
        expect.anything(),
      );
    });

    it('reuses existing CryptoKey from IndexedDB', async () => {
      const existingKey = await crypto.subtle.generateKey(
        { name: 'AES-GCM', length: 256 },
        false,
        ['encrypt', 'decrypt'],
      );

      // Return the existing key for the handle lookup
      mockGet.mockImplementation((key: string) => {
        if (key === '__emailibrium_crypto_key__') return Promise.resolve(existingKey);
        return Promise.resolve(undefined);
      });

      await secureStorage.setItem('foo', 'bar');

      // set() should only be called for the encrypted data, not for a new key
      const keySaveCalls = mockSet.mock.calls.filter(
        ([k]: [string]) => k === '__emailibrium_crypto_key__',
      );
      expect(keySaveCalls).toHaveLength(0);
    });
  });

  // -----------------------------------------------------------------------
  // Encrypt / Decrypt roundtrip
  // -----------------------------------------------------------------------

  describe('encrypt/decrypt roundtrip', () => {
    let storedData: Map<string, unknown>;

    beforeEach(() => {
      storedData = new Map();

      // Simulate IndexedDB with an in-memory map
      mockGet.mockImplementation((key: string) => Promise.resolve(storedData.get(key)));
      mockSet.mockImplementation((key: string, value: unknown) => {
        storedData.set(key, value);
        return Promise.resolve();
      });
      mockDel.mockImplementation((key: string) => {
        storedData.delete(key);
        return Promise.resolve();
      });
    });

    it('round-trips a simple string', async () => {
      await secureStorage.setItem('token', 'my-secret-value');
      const result = await secureStorage.getItem('token');
      expect(result).toBe('my-secret-value');
    });

    it('round-trips an empty string', async () => {
      await secureStorage.setItem('empty', '');
      const result = await secureStorage.getItem('empty');
      expect(result).toBe('');
    });

    it('round-trips unicode content', async () => {
      const unicode = 'Hello \u{1F600} \u4F60\u597D \u00E9\u00E0\u00FC';
      await secureStorage.setItem('unicode', unicode);
      const result = await secureStorage.getItem('unicode');
      expect(result).toBe(unicode);
    });

    it('encrypted data differs from plaintext', async () => {
      const plaintext = 'my-secret-value';
      await secureStorage.setItem('token', plaintext);

      const storedEncrypted = storedData.get('__emailibrium_secure_token__') as ArrayBuffer;
      expect(storedEncrypted).toBeInstanceOf(ArrayBuffer);

      const storedBytes = new Uint8Array(storedEncrypted);
      const plaintextBytes = new TextEncoder().encode(plaintext);

      // The stored data should NOT simply be the plaintext bytes
      const isSame =
        storedBytes.length === plaintextBytes.length &&
        storedBytes.every((b, i) => b === plaintextBytes[i]);
      expect(isSame).toBe(false);
    });
  });

  // -----------------------------------------------------------------------
  // getItem returns null for missing keys
  // -----------------------------------------------------------------------

  describe('getItem', () => {
    it('returns null when key does not exist', async () => {
      mockGet.mockResolvedValue(undefined);
      const result = await secureStorage.getItem('nonexistent');
      expect(result).toBeNull();
    });
  });

  // -----------------------------------------------------------------------
  // removeItem
  // -----------------------------------------------------------------------

  describe('removeItem', () => {
    it('delegates to del() with the namespaced key', async () => {
      await secureStorage.removeItem('token');
      expect(mockDel).toHaveBeenCalledWith('__emailibrium_secure_token__');
    });
  });

  // -----------------------------------------------------------------------
  // Error handling
  // -----------------------------------------------------------------------

  describe('error handling', () => {
    it('propagates decryption errors', async () => {
      // Return a valid key but garbage encrypted data
      const key = await crypto.subtle.generateKey(
        { name: 'AES-GCM', length: 256 },
        false,
        ['encrypt', 'decrypt'],
      );

      mockGet.mockImplementation((k: string) => {
        if (k === '__emailibrium_crypto_key__') return Promise.resolve(key);
        // Return garbage data that will fail decryption
        return Promise.resolve(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 99, 99]).buffer);
      });

      await expect(secureStorage.getItem('bad')).rejects.toThrow();
    });
  });
});
