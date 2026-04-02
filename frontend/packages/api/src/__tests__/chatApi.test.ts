import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

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
  sendChatMessage,
  createChatStream,
  streamChatMessage,
  getChatSessions,
  deleteChatSession,
} from '../chatApi.js';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('chatApi', () => {
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
    jsonFn.mockResolvedValue({});
    vi.stubGlobal('localStorage', mockLocalStorage);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  describe('sendChatMessage', () => {
    it('calls POST ai/chat with request JSON', async () => {
      const request = { message: 'Hello', sessionId: 's1' } as never;
      const response = { reply: 'Hi there', sessionId: 's1' };
      jsonFn.mockResolvedValueOnce(response);

      const result = await sendChatMessage(request);

      expect(mockPost).toHaveBeenCalledWith('ai/chat', { json: request });
      expect(result).toEqual(response);
    });
  });

  describe('createChatStream', () => {
    it('creates EventSource with correct URL containing message param', () => {
      const mockClose = vi.fn();
      class MockEventSource {
        url: string;
        close = mockClose;
        constructor(url: string, _opts?: Record<string, unknown>) {
          this.url = url;
          MockEventSource.instances.push(this);
        }
        static instances: MockEventSource[] = [];
      }
      vi.stubGlobal('EventSource', MockEventSource);

      const request = { message: 'What is this?' } as never;
      const { eventSource, abort } = createChatStream(request);

      const instance = MockEventSource.instances[0];
      expect(instance.url).toContain('/api/v1/ai/chat/stream?');
      const params = new URLSearchParams(instance.url.split('?')[1]);
      expect(params.get('message')).toBe('What is this?');

      expect(eventSource).toBeDefined();
      abort();
      expect(mockClose).toHaveBeenCalled();
    });

    it('includes sessionId in URL params when provided', () => {
      class MockEventSource {
        url: string;
        close = vi.fn();
        constructor(url: string, _opts?: Record<string, unknown>) {
          this.url = url;
          MockEventSource.instances.push(this);
        }
        static instances: MockEventSource[] = [];
      }
      vi.stubGlobal('EventSource', MockEventSource);

      const request = { message: 'Hi', sessionId: 'sess-42' } as never;
      createChatStream(request);

      const instance = MockEventSource.instances[0];
      const params = new URLSearchParams(instance.url.split('?')[1]);
      expect(params.get('sessionId')).toBe('sess-42');
    });
  });

  describe('streamChatMessage', () => {
    it('sends POST to /api/v1/ai/chat/stream with auth header', async () => {
      mockLocalStorage.getItem.mockReturnValue('my-token');

      const mockReader = {
        read: vi.fn().mockResolvedValueOnce({ done: true, value: undefined }),
      };
      const mockResponse = {
        ok: true,
        body: { getReader: () => mockReader },
      };
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse));

      const onChunk = vi.fn();
      const onDone = vi.fn();
      const onError = vi.fn();

      await streamChatMessage({ message: 'test' } as never, onChunk, onDone, onError);

      const fetchCall = (globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls[0];
      expect(fetchCall[0]).toBe('/api/v1/ai/chat/stream');
      expect(fetchCall[1].method).toBe('POST');
      expect(fetchCall[1].headers.Authorization).toBe('Bearer my-token');
      expect(fetchCall[1].headers['Content-Type']).toBe('application/json');

      vi.unstubAllGlobals();
    });

    it('calls onError when response is not ok', async () => {
      mockLocalStorage.getItem.mockReturnValue(null);

      vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 403 }));

      const onChunk = vi.fn();
      const onDone = vi.fn();
      const onError = vi.fn();

      const abort = await streamChatMessage({ message: 'test' } as never, onChunk, onDone, onError);

      expect(onError).toHaveBeenCalledWith('Chat request failed: 403');
      expect(typeof abort).toBe('function');

      vi.unstubAllGlobals();
    });
  });

  describe('getChatSessions', () => {
    it('calls GET ai/chat/sessions', async () => {
      const sessions = [{ id: 's1' }, { id: 's2' }];
      jsonFn.mockResolvedValueOnce(sessions);

      const result = await getChatSessions();

      expect(mockGet).toHaveBeenCalledWith('ai/chat/sessions');
      expect(result).toEqual(sessions);
    });
  });

  describe('deleteChatSession', () => {
    it('calls DELETE ai/chat/sessions/:sessionId', async () => {
      await deleteChatSession('s1');
      expect(mockDelete).toHaveBeenCalledWith('ai/chat/sessions/s1');
    });
  });
});
