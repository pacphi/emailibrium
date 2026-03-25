import type { ChatRequest, ChatResponse, ChatSession } from '@emailibrium/types';
import { api } from './client.js';

export async function sendChatMessage(request: ChatRequest): Promise<ChatResponse> {
  return api.post('ai/chat', { json: request }).json<ChatResponse>();
}

/**
 * Creates an SSE connection for streaming chat responses.
 * Returns an EventSource-compatible interface that emits tokens
 * as they arrive from the backend.
 */
export function createChatStream(request: ChatRequest): {
  eventSource: EventSource;
  abort: () => void;
} {
  const params = new URLSearchParams();
  params.set('message', request.message);
  if (request.sessionId) {
    params.set('sessionId', request.sessionId);
  }
  if (request.history) {
    params.set('history', JSON.stringify(request.history));
  }

  const url = `/api/v1/ai/chat/stream?${params.toString()}`;
  const eventSource = new EventSource(url, { withCredentials: true });

  return {
    eventSource,
    abort: () => eventSource.close(),
  };
}

/**
 * Alternative streaming approach using fetch + ReadableStream
 * for POST-based SSE when EventSource (GET-only) is insufficient.
 */
export async function streamChatMessage(
  request: ChatRequest,
  onChunk: (chunk: string) => void,
  onDone: (sessionId: string) => void,
  onError: (error: string) => void,
): Promise<() => void> {
  const controller = new AbortController();
  const token = localStorage.getItem('auth_token');

  try {
    const response = await fetch('/api/v1/ai/chat/stream', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: JSON.stringify(request),
      signal: controller.signal,
    });

    if (!response.ok) {
      onError(`Chat request failed: ${response.status}`);
      return () => controller.abort();
    }

    const reader = response.body?.getReader();
    if (!reader) {
      onError('No response body');
      return () => controller.abort();
    }

    const decoder = new TextDecoder();
    let buffer = '';

    (async () => {
      try {
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;

          buffer += decoder.decode(value, { stream: true });
          const lines = buffer.split('\n');
          buffer = lines.pop() ?? '';

          for (const line of lines) {
            if (line.startsWith('data: ')) {
              const data = line.slice(6).trim();
              if (data === '[DONE]') {
                continue;
              }
              try {
                const parsed = JSON.parse(data);
                if (parsed.type === 'token' && parsed.content) {
                  onChunk(parsed.content);
                } else if (parsed.type === 'done' && parsed.sessionId) {
                  onDone(parsed.sessionId);
                } else if (parsed.type === 'error') {
                  onError(parsed.error ?? 'Unknown streaming error');
                }
              } catch {
                // Non-JSON SSE line, treat as raw token
                if (data) onChunk(data);
              }
            }
          }
        }
      } catch (err) {
        if ((err as Error).name !== 'AbortError') {
          onError((err as Error).message);
        }
      }
    })();
  } catch (err) {
    if ((err as Error).name !== 'AbortError') {
      onError((err as Error).message);
    }
  }

  return () => controller.abort();
}

export async function getChatSessions(): Promise<ChatSession[]> {
  return api.get('ai/chat/sessions').json<ChatSession[]>();
}

export async function deleteChatSession(sessionId: string): Promise<void> {
  await api.delete(`ai/chat/sessions/${sessionId}`);
}
