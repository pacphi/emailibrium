import { useState, useCallback } from 'react';
import { api } from '@emailibrium/api';
import type { RuleSuggestion } from '@emailibrium/types';

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  timestamp: string;
  suggestions?: RuleSuggestion[];
}

interface ChatResponse {
  message: string;
  suggestions?: RuleSuggestion[];
}

function generateId(): string {
  return `msg_${Date.now()}_${Math.random().toString(36).slice(2, 9)}`;
}

/**
 * Manages chat state for the conversational rule-building assistant.
 * Handles message history, API communication, and loading state.
 */
export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isLoading, setIsLoading] = useState(false);

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

      try {
        const response = await api
          .post('chat', {
            json: {
              message: trimmed,
              history: messages.map(({ role, content }) => ({ role, content })),
            },
          })
          .json<ChatResponse>();

        const assistantMessage: ChatMessage = {
          id: generateId(),
          role: 'assistant',
          content: response.message,
          timestamp: new Date().toISOString(),
          suggestions: response.suggestions,
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
    },
    [isLoading, messages],
  );

  const clearHistory = useCallback(() => {
    setMessages([]);
  }, []);

  return { messages, isLoading, sendMessage, clearHistory };
}
