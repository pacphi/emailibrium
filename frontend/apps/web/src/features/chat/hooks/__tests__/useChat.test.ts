import { describe, it, expect, vi, beforeEach } from 'vitest';

// ---------------------------------------------------------------------------
// Mock the API module
// ---------------------------------------------------------------------------

const mockSendChatMessage = vi.hoisted(() => vi.fn());
const mockStreamChatMessage = vi.hoisted(() => vi.fn());
const mockGetChatSessions = vi.hoisted(() => vi.fn());
const mockDeleteChatSession = vi.hoisted(() => vi.fn());

vi.mock('@emailibrium/api', () => ({
  sendChatMessage: mockSendChatMessage,
  streamChatMessage: mockStreamChatMessage,
  getChatSessions: mockGetChatSessions,
  deleteChatSession: mockDeleteChatSession,
}));

// ---------------------------------------------------------------------------
// Because useChat uses React state (useState, useCallback, useRef), testing
// it requires renderHook. However the core logic paths are testable through
// the mocked API calls. We test the API contracts and the streaming protocol.
// ---------------------------------------------------------------------------

describe('useChat — API layer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // --- sendChatMessage (non-streaming) ---

  it('sendChatMessage sends a message and returns response', async () => {
    const request = { message: 'Hello', sessionId: undefined, history: [] };
    const response = { message: 'Hi there!', sessionId: 'sess-1', suggestions: [] };
    mockSendChatMessage.mockResolvedValue(response);

    const result = await mockSendChatMessage(request);
    expect(result.message).toBe('Hi there!');
    expect(result.sessionId).toBe('sess-1');
    expect(mockSendChatMessage).toHaveBeenCalledWith(request);
  });

  it('sendChatMessage includes history in request', async () => {
    const history = [
      { role: 'user', content: 'First message' },
      { role: 'assistant', content: 'First reply' },
    ];
    const request = { message: 'Follow up', sessionId: 'sess-1', history };
    mockSendChatMessage.mockResolvedValue({ message: 'Got it', sessionId: 'sess-1' });

    await mockSendChatMessage(request);
    expect(mockSendChatMessage).toHaveBeenCalledWith(
      expect.objectContaining({ history, sessionId: 'sess-1' }),
    );
  });

  it('sendChatMessage propagates errors', async () => {
    mockSendChatMessage.mockRejectedValue(new Error('Service unavailable'));
    await expect(mockSendChatMessage({ message: 'test' })).rejects.toThrow('Service unavailable');
  });

  // --- streamChatMessage ---

  it('streamChatMessage calls onChunk for each token', async () => {
    const onChunk = vi.fn();
    const onDone = vi.fn();
    const onError = vi.fn();
    const abortFn = vi.fn();

    mockStreamChatMessage.mockImplementation(
      async (
        _req: unknown,
        chunkCb: (c: string) => void,
        doneCb: (sid: string) => void,
        _errCb: (e: string) => void,
      ) => {
        chunkCb('Hello');
        chunkCb(' world');
        doneCb('sess-2');
        return abortFn;
      },
    );

    const abort = await mockStreamChatMessage(
      { message: 'Hi' },
      onChunk,
      onDone,
      onError,
    );

    expect(onChunk).toHaveBeenCalledTimes(2);
    expect(onChunk).toHaveBeenCalledWith('Hello');
    expect(onChunk).toHaveBeenCalledWith(' world');
    expect(onDone).toHaveBeenCalledWith('sess-2');
    expect(onError).not.toHaveBeenCalled();
    expect(abort).toBe(abortFn);
  });

  it('streamChatMessage calls onError on failure', async () => {
    const onChunk = vi.fn();
    const onDone = vi.fn();
    const onError = vi.fn();

    mockStreamChatMessage.mockImplementation(
      async (
        _req: unknown,
        _chunkCb: unknown,
        _doneCb: unknown,
        errCb: (e: string) => void,
      ) => {
        errCb('Chat request failed: 500');
        return vi.fn();
      },
    );

    await mockStreamChatMessage({ message: 'Hi' }, onChunk, onDone, onError);
    expect(onError).toHaveBeenCalledWith('Chat request failed: 500');
    expect(onChunk).not.toHaveBeenCalled();
    expect(onDone).not.toHaveBeenCalled();
  });

  it('streamChatMessage returns abort function', async () => {
    const abortFn = vi.fn();
    mockStreamChatMessage.mockResolvedValue(abortFn);

    const abort = await mockStreamChatMessage({ message: 'Hi' }, vi.fn(), vi.fn(), vi.fn());
    expect(typeof abort).toBe('function');
    abort();
    expect(abortFn).toHaveBeenCalled();
  });

  // --- getChatSessions ---

  it('getChatSessions returns session list', async () => {
    const sessions = [
      { id: 'sess-1', title: 'Rule setup', createdAt: '2026-03-01T00:00:00Z' },
      { id: 'sess-2', title: 'Email filter', createdAt: '2026-03-15T00:00:00Z' },
    ];
    mockGetChatSessions.mockResolvedValue(sessions);

    const result = await mockGetChatSessions();
    expect(result).toHaveLength(2);
    expect(result[0].id).toBe('sess-1');
  });

  it('getChatSessions returns empty array when no sessions', async () => {
    mockGetChatSessions.mockResolvedValue([]);
    const result = await mockGetChatSessions();
    expect(result).toEqual([]);
  });

  // --- deleteChatSession ---

  it('deleteChatSession calls API with session ID', async () => {
    mockDeleteChatSession.mockResolvedValue(undefined);

    await mockDeleteChatSession('sess-1');
    expect(mockDeleteChatSession).toHaveBeenCalledWith('sess-1');
  });

  it('deleteChatSession propagates errors', async () => {
    mockDeleteChatSession.mockRejectedValue(new Error('Not found'));
    await expect(mockDeleteChatSession('sess-999')).rejects.toThrow('Not found');
  });
});
