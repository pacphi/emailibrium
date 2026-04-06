import { useState, useCallback, useRef } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  sendChatMessage,
  streamChatMessage,
  confirmToolCall,
  getChatSessions,
  deleteChatSession,
} from '@emailibrium/api';
import type {
  RuleSuggestion,
  ChatSession,
  ToolCallEvent,
  ConfirmationEvent,
} from '@emailibrium/types';

export interface ToolCallStatus {
  id: string;
  name: string;
  status: 'calling' | 'complete' | 'error';
}

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  timestamp: string;
  suggestions?: RuleSuggestion[];
  isStreaming?: boolean;
  toolCalls?: ToolCallStatus[];
}

function generateId(): string {
  return `msg_${Date.now()}_${Math.random().toString(36).slice(2, 9)}`;
}

/**
 * Manages chat state for the conversational rule-building assistant.
 * Supports both standard request/response and SSE streaming modes.
 *
 * All inference runs on the Rust backend (Tier 0.5 built-in LLM, Ollama,
 * or cloud) -- the frontend is a pure REST/SSE client.
 */
export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [sessionId, setSessionId] = useState<string | undefined>(undefined);
  const [streamingEnabled, setStreamingEnabled] = useState(true);
  const [pendingToolCalls, setPendingToolCalls] = useState<ToolCallEvent[]>([]);
  const [pendingConfirmation, setPendingConfirmation] = useState<ConfirmationEvent | null>(null);
  const abortRef = useRef<(() => void) | null>(null);

  const sendMessage = useCallback(
    async (text: string): Promise<void> => {
      const trimmed = text.trim();
      if (!trimmed || isLoading) return;

      const userMessage: ChatMessage = {
        id: generateId(),
        role: 'user',
        content: trimmed,
        timestamp: new Date().toISOString(),
      };

      setMessages((prev) => [...prev, userMessage]);
      setIsLoading(true);

      const history = messages.map(({ role, content }) => ({ role, content }));
      const request = { message: trimmed, sessionId, history };

      if (streamingEnabled) {
        const assistantId = generateId();
        setMessages((prev) => [
          ...prev,
          {
            id: assistantId,
            role: 'assistant',
            content: '',
            timestamp: new Date().toISOString(),
            isStreaming: true,
          },
        ]);

        try {
          const abort = await streamChatMessage(request, {
            onChunk: (chunk) => {
              setMessages((prev) =>
                prev.map((m) => (m.id === assistantId ? { ...m, content: m.content + chunk } : m)),
              );
            },
            onDone: (newSessionId) => {
              setSessionId(newSessionId);
              setPendingToolCalls([]);
              setMessages((prev) =>
                prev.map((m) => (m.id === assistantId ? { ...m, isStreaming: false } : m)),
              );
              setIsLoading(false);
            },
            onError: (error) => {
              setMessages((prev) =>
                prev.map((m) =>
                  m.id === assistantId
                    ? {
                        ...m,
                        content: m.content || `Sorry, an error occurred: ${error}`,
                        isStreaming: false,
                      }
                    : m,
                ),
              );
              setIsLoading(false);
            },
            onToolCall: (toolCall) => {
              setPendingToolCalls((prev) => [...prev, toolCall]);
              setMessages((prev) =>
                prev.map((m) =>
                  m.id === assistantId
                    ? {
                        ...m,
                        toolCalls: [
                          ...(m.toolCalls ?? []),
                          { id: toolCall.id, name: toolCall.name, status: 'calling' as const },
                        ],
                      }
                    : m,
                ),
              );
            },
            onToolResult: (toolResult) => {
              setPendingToolCalls((prev) => prev.filter((tc) => tc.id !== toolResult.toolCallId));
              setMessages((prev) =>
                prev.map((m) =>
                  m.id === assistantId
                    ? {
                        ...m,
                        toolCalls: m.toolCalls?.map((tc) =>
                          tc.id === toolResult.toolCallId
                            ? {
                                ...tc,
                                status: (toolResult.isError ? 'error' : 'complete') as const,
                              }
                            : tc,
                        ),
                      }
                    : m,
                ),
              );
            },
            onConfirmation: (confirmation) => {
              setPendingConfirmation(confirmation);
            },
          });
          abortRef.current = abort;
        } catch {
          setMessages((prev) =>
            prev.map((m) =>
              m.id === assistantId
                ? {
                    ...m,
                    content: 'Sorry, I encountered an error. Please try again.',
                    isStreaming: false,
                  }
                : m,
            ),
          );
          setIsLoading(false);
        }
      } else {
        // Non-streaming fallback
        try {
          const response = await sendChatMessage(request);
          setSessionId(response.sessionId);

          const assistantMessage: ChatMessage = {
            id: generateId(),
            role: 'assistant',
            content: response.message,
            timestamp: new Date().toISOString(),
            suggestions: response.suggestions as RuleSuggestion[] | undefined,
          };

          setMessages((prev) => [...prev, assistantMessage]);
        } catch {
          const errorMessage: ChatMessage = {
            id: generateId(),
            role: 'assistant',
            content: 'Sorry, I encountered an error. Please try again.',
            timestamp: new Date().toISOString(),
          };

          setMessages((prev) => [...prev, errorMessage]);
        } finally {
          setIsLoading(false);
        }
      }
    },
    [isLoading, messages, sessionId, streamingEnabled],
  );

  const stopStreaming = useCallback(() => {
    abortRef.current?.();
    abortRef.current = null;
    setMessages((prev) => prev.map((m) => (m.isStreaming ? { ...m, isStreaming: false } : m)));
    setIsLoading(false);
  }, []);

  const handleConfirmation = useCallback(
    async (approved: boolean): Promise<void> => {
      if (!pendingConfirmation || !sessionId) return;
      try {
        await confirmToolCall(sessionId, pendingConfirmation.confirmationId, approved);
      } catch {
        // Error will surface through the SSE stream
      } finally {
        setPendingConfirmation(null);
      }
    },
    [pendingConfirmation, sessionId],
  );

  const clearHistory = useCallback(() => {
    abortRef.current?.();
    abortRef.current = null;
    setMessages([]);
    setSessionId(undefined);
    setPendingToolCalls([]);
    setPendingConfirmation(null);
    setIsLoading(false);
  }, []);

  return {
    messages,
    isLoading,
    sessionId,
    streamingEnabled,
    setStreamingEnabled,
    pendingToolCalls,
    pendingConfirmation,
    sendMessage,
    stopStreaming,
    clearHistory,
    handleConfirmation,
  };
}

/**
 * React Query hook for fetching chat session history.
 */
export function useChatSessions() {
  return useQuery<ChatSession[]>({
    queryKey: ['chatSessions'],
    queryFn: getChatSessions,
    staleTime: 30_000,
  });
}

/**
 * Mutation hook for deleting a chat session.
 */
export function useDeleteChatSession() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteChatSession,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['chatSessions'] });
    },
  });
}
