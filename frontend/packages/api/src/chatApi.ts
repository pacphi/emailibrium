import type {
  ChatRequest,
  ChatResponse,
  ChatSession,
  ToolCallEvent,
  ToolResultEvent,
  ConfirmationEvent,
} from '@emailibrium/types';
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
 * Send a tool confirmation response (approve or reject a pending tool call).
 */
export async function confirmToolCall(
  sessionId: string,
  confirmationId: string,
  approved: boolean,
): Promise<void> {
  const token = localStorage.getItem('auth_token');
  const response = await fetch('/api/v1/ai/chat/confirm', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify({ sessionId, confirmationId, approved }),
  });
  if (!response.ok) {
    throw new Error(`Confirmation failed: ${response.statusText}`);
  }
}

export interface StreamChatCallbacks {
  onChunk: (chunk: string) => void;
  onDone: (sessionId: string) => void;
  onError: (error: string) => void;
  onToolCall?: (toolCall: ToolCallEvent) => void;
  onToolResult?: (toolResult: ToolResultEvent) => void;
  onConfirmation?: (confirmation: ConfirmationEvent) => void;
}

/**
 * Streaming chat using fetch + ReadableStream for POST-based SSE.
 *
 * Supports tool-calling events (tool_call, tool_result, confirmation)
 * in addition to the existing token/done/error events.
 *
 * @overload Legacy signature for backward compatibility
 */
export async function streamChatMessage(
  request: ChatRequest,
  onChunk: (chunk: string) => void,
  onDone: (sessionId: string) => void,
  onError: (error: string) => void,
): Promise<() => void>;
/**
 * @overload Extended signature with tool-calling callbacks
 */
export async function streamChatMessage(
  request: ChatRequest,
  callbacks: StreamChatCallbacks,
): Promise<() => void>;
export async function streamChatMessage(
  request: ChatRequest,
  onChunkOrCallbacks: ((chunk: string) => void) | StreamChatCallbacks,
  onDone?: (sessionId: string) => void,
  onError?: (error: string) => void,
): Promise<() => void> {
  const callbacks: StreamChatCallbacks =
    typeof onChunkOrCallbacks === 'function'
      ? { onChunk: onChunkOrCallbacks, onDone: onDone!, onError: onError! }
      : onChunkOrCallbacks;
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
      callbacks.onError(`Chat request failed: ${response.status}`);
      return () => controller.abort();
    }

    const reader = response.body?.getReader();
    if (!reader) {
      callbacks.onError('No response body');
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
                  callbacks.onChunk(parsed.content);
                } else if (parsed.type === 'done' && parsed.sessionId) {
                  callbacks.onDone(parsed.sessionId);
                } else if (parsed.type === 'error') {
                  callbacks.onError(parsed.error ?? 'Unknown streaming error');
                } else if (parsed.type === 'tool_call' && parsed.toolCall) {
                  callbacks.onToolCall?.(parsed.toolCall);
                } else if (parsed.type === 'tool_result' && parsed.toolResult) {
                  callbacks.onToolResult?.(parsed.toolResult);
                } else if (parsed.type === 'confirmation' && parsed.confirmation) {
                  callbacks.onConfirmation?.(parsed.confirmation);
                }
              } catch {
                // Non-JSON SSE line, treat as raw token
                if (data) callbacks.onChunk(data);
              }
            }
          }
        }
      } catch (err) {
        if ((err as Error).name !== 'AbortError') {
          callbacks.onError((err as Error).message);
        }
      }
    })();
  } catch (err) {
    if ((err as Error).name !== 'AbortError') {
      callbacks.onError((err as Error).message);
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
