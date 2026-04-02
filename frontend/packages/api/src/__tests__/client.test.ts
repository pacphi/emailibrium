import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// ---------------------------------------------------------------------------
// Mocks (vitest 4: hoist mock variables)
// ---------------------------------------------------------------------------

const { mockCreate, mockGet, mockPost, mockDelete, mockPatch, mockPut, capturedHooks } = vi.hoisted(
  () => {
    const jsonFn = vi.fn().mockResolvedValue({});
    const responseLike = { json: jsonFn };

    const mockGet = vi.fn().mockReturnValue(responseLike);
    const mockPost = vi.fn().mockReturnValue(responseLike);
    const mockDelete = vi.fn().mockReturnValue(responseLike);
    const mockPatch = vi.fn().mockReturnValue(responseLike);
    const mockPut = vi.fn().mockReturnValue(responseLike);

    const capturedHooks: { beforeRequest: Array<(req: Request) => void> } = {
      beforeRequest: [],
    };

    const mockInstance = {
      get: mockGet,
      post: mockPost,
      delete: mockDelete,
      patch: mockPatch,
      put: mockPut,
    };

    const mockCreate = vi.fn().mockImplementation((options: Record<string, unknown>) => {
      if (options?.hooks) {
        const hooks = options.hooks as { beforeRequest?: Array<(req: Request) => void> };
        capturedHooks.beforeRequest = hooks.beforeRequest ?? [];
      }
      return mockInstance;
    });

    return { mockCreate, mockGet, mockPost, mockDelete, mockPatch, mockPut, capturedHooks };
  },
);

vi.mock('ky', () => ({
  default: { create: mockCreate },
}));

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('client', () => {
  const mockLocalStorage = {
    getItem: vi.fn(),
    setItem: vi.fn(),
    removeItem: vi.fn(),
    clear: vi.fn(),
    length: 0,
    key: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal('localStorage', mockLocalStorage);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  // -----------------------------------------------------------------------
  // ky.create configuration
  // -----------------------------------------------------------------------

  describe('ky.create configuration', () => {
    it('calls ky.create with correct prefixUrl', async () => {
      // Re-import to trigger ky.create
      vi.resetModules();
      await import('../client.js');

      expect(mockCreate).toHaveBeenCalledWith(expect.objectContaining({ prefixUrl: '/api/v1' }));
    });

    it('configures a 30-second timeout', async () => {
      vi.resetModules();
      await import('../client.js');

      expect(mockCreate).toHaveBeenCalledWith(expect.objectContaining({ timeout: 30_000 }));
    });

    it('registers a beforeRequest hook', async () => {
      vi.resetModules();
      await import('../client.js');

      const call = mockCreate.mock.calls[0]?.[0] as Record<string, unknown> | undefined;
      const hooks = call?.hooks as { beforeRequest?: unknown[] } | undefined;
      expect(hooks?.beforeRequest).toBeDefined();
      expect(hooks!.beforeRequest!.length).toBeGreaterThan(0);
    });

    it('exports an api instance', async () => {
      vi.resetModules();
      const mod = await import('../client.js');
      expect(mod.api).toBeDefined();
    });
  });

  // -----------------------------------------------------------------------
  // Auth header injection
  // -----------------------------------------------------------------------

  describe('auth header injection (beforeRequest hook)', () => {
    it('sets Authorization header when token exists', async () => {
      vi.resetModules();
      await import('../client.js');

      mockLocalStorage.getItem.mockReturnValue('test-jwt-token');

      const hook = capturedHooks.beforeRequest[0];
      expect(hook).toBeDefined();

      const fakeRequest = new Request('http://localhost/api/v1/test');
      hook(fakeRequest);

      expect(fakeRequest.headers.get('Authorization')).toBe('Bearer test-jwt-token');
    });

    it('does not set Authorization header when no token', async () => {
      vi.resetModules();
      await import('../client.js');

      mockLocalStorage.getItem.mockReturnValue(null);

      const hook = capturedHooks.beforeRequest[0];
      const fakeRequest = new Request('http://localhost/api/v1/test');
      hook(fakeRequest);

      expect(fakeRequest.headers.get('Authorization')).toBeNull();
    });

    it('does not crash when localStorage is empty', async () => {
      vi.resetModules();
      await import('../client.js');

      mockLocalStorage.getItem.mockReturnValue(null);

      const hook = capturedHooks.beforeRequest[0];
      const fakeRequest = new Request('http://localhost/api/v1/test');

      expect(() => hook(fakeRequest)).not.toThrow();
    });

    it('reads from the auth_token key in localStorage', async () => {
      vi.resetModules();
      await import('../client.js');

      mockLocalStorage.getItem.mockReturnValue('abc');

      const hook = capturedHooks.beforeRequest[0];
      const fakeRequest = new Request('http://localhost/api/v1/test');
      hook(fakeRequest);

      expect(mockLocalStorage.getItem).toHaveBeenCalledWith('auth_token');
    });

    it('formats token as Bearer scheme', async () => {
      vi.resetModules();
      await import('../client.js');

      mockLocalStorage.getItem.mockReturnValue('xyz123');

      const hook = capturedHooks.beforeRequest[0];
      const fakeRequest = new Request('http://localhost/api/v1/test');
      hook(fakeRequest);

      const header = fakeRequest.headers.get('Authorization');
      expect(header).toMatch(/^Bearer /);
      expect(header).toBe('Bearer xyz123');
    });
  });

  // -----------------------------------------------------------------------
  // Error response handling
  // -----------------------------------------------------------------------

  describe('error response handling', () => {
    it('propagates 4xx errors from ky', async () => {
      vi.resetModules();
      const { api } = await import('../client.js');

      const error = new Error('Request failed with status 404');
      mockGet.mockReturnValueOnce({
        json: vi.fn().mockRejectedValue(error),
      });

      await expect(api.get('test').json()).rejects.toThrow('Request failed with status 404');
    });

    it('propagates 5xx errors from ky', async () => {
      vi.resetModules();
      const { api } = await import('../client.js');

      const error = new Error('Request failed with status 500');
      mockGet.mockReturnValueOnce({
        json: vi.fn().mockRejectedValue(error),
      });

      await expect(api.get('test').json()).rejects.toThrow('Request failed with status 500');
    });

    it('propagates network errors', async () => {
      vi.resetModules();
      const { api } = await import('../client.js');

      const error = new TypeError('Failed to fetch');
      mockPost.mockReturnValueOnce({
        json: vi.fn().mockRejectedValue(error),
      });

      await expect(api.post('test').json()).rejects.toThrow('Failed to fetch');
    });

    it('propagates timeout errors', async () => {
      vi.resetModules();
      const { api } = await import('../client.js');

      const error = new Error('Request timed out');
      mockGet.mockReturnValueOnce({
        json: vi.fn().mockRejectedValue(error),
      });

      await expect(api.get('test').json()).rejects.toThrow('Request timed out');
    });
  });
});
