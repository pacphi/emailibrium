import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// ---------------------------------------------------------------------------
// Mocks (vitest 4: hoist mock variables)
// ---------------------------------------------------------------------------

const { mockCreate, mockGet, mockPost, capturedHooks } = vi.hoisted(() => {
  const jsonFn = vi.fn().mockResolvedValue({});
  const responseLike = { json: jsonFn };

  const mockGet = vi.fn().mockReturnValue(responseLike);
  const mockPost = vi.fn().mockReturnValue(responseLike);
  const mockDelete = vi.fn().mockReturnValue(responseLike);
  const mockPatch = vi.fn().mockReturnValue(responseLike);
  const mockPut = vi.fn().mockReturnValue(responseLike);

  type BeforeRequestState = { request: Request; options: unknown; retryCount: 0 };
  const capturedHooks: {
    beforeRequest: Array<(state: BeforeRequestState) => void>;
    afterResponse: Array<(state: { response: Response }) => void>;
  } = {
    beforeRequest: [],
    afterResponse: [],
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
      const hooks = options.hooks as {
        beforeRequest?: Array<(state: BeforeRequestState) => void>;
        afterResponse?: Array<(state: { response: Response }) => void>;
      };
      capturedHooks.beforeRequest = hooks.beforeRequest ?? [];
      capturedHooks.afterResponse = hooks.afterResponse ?? [];
    }
    return mockInstance;
  });

  return { mockCreate, mockGet, mockPost, mockDelete, mockPatch, mockPut, capturedHooks };
});

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

      expect(mockCreate).toHaveBeenCalledWith(expect.objectContaining({ prefix: '/api/v1' }));
    });

    it('configures a 30-second timeout', async () => {
      vi.resetModules();
      await import('../client.js');

      expect(mockCreate).toHaveBeenCalledWith(expect.objectContaining({ timeout: 30_000 }));
    });

    it('registers an authentication-response hook', async () => {
      vi.resetModules();
      await import('../client.js');

      const call = mockCreate.mock.calls[0]?.[0] as Record<string, unknown> | undefined;
      const hooks = call?.hooks as { afterResponse?: unknown[] } | undefined;
      expect(hooks?.afterResponse).toBeDefined();
      expect(hooks!.afterResponse!.length).toBeGreaterThan(0);
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

  describe('session cookie authentication', () => {
    it('includes same-origin session credentials', async () => {
      vi.resetModules();
      await import('../client.js');
      expect(mockCreate).toHaveBeenCalledWith(
        expect.objectContaining({ credentials: 'same-origin' }),
      );
    });

    it('never reads a persistent root access token for API requests', async () => {
      vi.resetModules();
      await import('../client.js');
      const request = new Request('http://localhost/api/v1/test');
      for (const hook of capturedHooks.beforeRequest) hook({ request, options: {}, retryCount: 0 });
      expect(mockLocalStorage.getItem).not.toHaveBeenCalled();
      expect(request.headers.get('Authorization')).toBeNull();
    });

    it('notifies the app when a private API request loses its session', async () => {
      const dispatchEvent = vi.fn();
      vi.stubGlobal('window', { dispatchEvent });
      vi.resetModules();
      await import('../client.js');
      capturedHooks.afterResponse[0]!({ response: new Response('{}', { status: 401 }) });
      expect(dispatchEvent).toHaveBeenCalledWith(
        expect.objectContaining({ type: 'emailibrium:unauthorized' }),
      );
      dispatchEvent.mockClear();
      capturedHooks.afterResponse[0]!({ response: new Response('{}', { status: 200 }) });
      expect(dispatchEvent).not.toHaveBeenCalled();
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
