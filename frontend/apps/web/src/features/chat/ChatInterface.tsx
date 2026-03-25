import { useRef, useEffect, useCallback } from 'react';
import { MessageSquare, Trash2, Loader2, Square } from 'lucide-react';
import type { RuleSuggestion } from '@emailibrium/types';
import { useCreateRule } from '@/features/rules/hooks/useRules';
import { useChat } from './hooks/useChat';
import { ChatMessage } from './ChatMessage';
import { ChatInput } from './ChatInput';

/**
 * Full-height chat interface for the conversational rule-building
 * AI assistant. Displays message history, typing indicators, and
 * action buttons for applying suggested rules. Supports SSE streaming.
 */
export function ChatInterface() {
  const { messages, isLoading, streamingEnabled, sendMessage, stopStreaming, clearHistory } =
    useChat();
  const createRule = useCreateRule();
  const scrollRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom when new messages arrive
  useEffect(() => {
    const container = scrollRef.current;
    if (container) {
      container.scrollTop = container.scrollHeight;
    }
  }, [messages, isLoading]);

  const handleApplyRule = useCallback(
    (suggestion: RuleSuggestion) => {
      createRule.mutate({
        name: suggestion.rule.name,
        conditions: suggestion.rule.conditions,
        actions: suggestion.rule.actions,
        isActive: true,
      });
    },
    [createRule],
  );

  const handleEditRule = useCallback((_suggestion: RuleSuggestion) => {
    // Navigate to rule editor with pre-filled data
  }, []);

  const handleFindSimilar = useCallback((_suggestion: RuleSuggestion) => {
    // Trigger a search for similar rules
  }, []);

  const isStreaming = messages.some((m) => m.isStreaming);

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="flex items-center justify-between border-b border-gray-200 px-4 py-3 dark:border-gray-700">
        <div className="flex items-center gap-2">
          <MessageSquare className="h-5 w-5 text-indigo-500" aria-hidden="true" />
          <h2 className="text-sm font-semibold text-gray-900 dark:text-white">Email Assistant</h2>
          {streamingEnabled && (
            <span className="rounded-full bg-green-100 px-1.5 py-0.5 text-[10px] font-medium text-green-700 dark:bg-green-900/30 dark:text-green-400">
              Live
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          {isStreaming && (
            <button
              type="button"
              onClick={stopStreaming}
              className="flex items-center gap-1 rounded-md bg-red-50 px-2 py-1 text-xs text-red-600 transition-colors hover:bg-red-100 dark:bg-red-900/20 dark:text-red-400 dark:hover:bg-red-900/30"
              aria-label="Stop generating"
            >
              <Square className="h-3 w-3" aria-hidden="true" />
              Stop
            </button>
          )}
          {messages.length > 0 && !isStreaming && (
            <button
              type="button"
              onClick={clearHistory}
              className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-gray-400 transition-colors hover:text-gray-600 dark:hover:text-gray-300"
              aria-label="Clear chat history"
            >
              <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
              Clear
            </button>
          )}
        </div>
      </div>

      {/* Message list */}
      <div
        ref={scrollRef}
        className="flex-1 overflow-y-auto px-4 py-4"
        role="list"
        aria-label="Chat messages"
      >
        {messages.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center text-gray-400 dark:text-gray-500">
            <MessageSquare className="mb-3 h-10 w-10" aria-hidden="true" />
            <p className="text-sm font-medium">Start a conversation</p>
            <p className="mt-1 max-w-xs text-center text-xs">
              Ask me to help organize your inbox, create rules, or find patterns in your emails.
            </p>
          </div>
        ) : (
          <div className="space-y-4">
            {messages.map((message) => (
              <ChatMessage
                key={message.id}
                message={message}
                onApplyRule={handleApplyRule}
                onEditRule={handleEditRule}
                onFindSimilar={handleFindSimilar}
              />
            ))}

            {/* Typing indicator (non-streaming mode) */}
            {isLoading && !isStreaming && (
              <div className="flex items-center gap-2 text-gray-400 dark:text-gray-500">
                <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                <span className="text-xs" role="status">
                  Assistant is typing...
                </span>
              </div>
            )}
          </div>
        )}
      </div>

      {/* Input */}
      <ChatInput onSend={sendMessage} disabled={isLoading && !isStreaming} />
    </div>
  );
}
