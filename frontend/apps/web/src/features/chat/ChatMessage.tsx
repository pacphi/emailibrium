import { Bot, User, Play, Pencil, Search } from 'lucide-react';
import type { RuleSuggestion } from '@emailibrium/types';
import type { ChatMessage as ChatMessageType } from './hooks/useChat';

interface ChatMessageProps {
  message: ChatMessageType;
  onApplyRule?: (suggestion: RuleSuggestion) => void;
  onEditRule?: (suggestion: RuleSuggestion) => void;
  onFindSimilar?: (suggestion: RuleSuggestion) => void;
}

function formatTime(timestamp: string): string {
  return new Date(timestamp).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
  });
}

/**
 * Extracts YAML code blocks from message content and returns
 * segments of plain text and code blocks for rendering.
 */
function parseContent(content: string): Array<{ type: 'text' | 'code'; value: string }> {
  const segments: Array<{ type: 'text' | 'code'; value: string }> = [];
  const codeBlockRegex = /```(?:yaml|yml)?\n([\s\S]*?)```/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = codeBlockRegex.exec(content)) !== null) {
    if (match.index > lastIndex) {
      segments.push({ type: 'text', value: content.slice(lastIndex, match.index) });
    }
    segments.push({ type: 'code', value: match[1] ?? '' });
    lastIndex = match.index + match[0].length;
  }

  if (lastIndex < content.length) {
    segments.push({ type: 'text', value: content.slice(lastIndex) });
  }

  if (segments.length === 0) {
    segments.push({ type: 'text', value: content });
  }

  return segments;
}

function SuggestionActions({
  suggestion,
  onApply,
  onEdit,
  onFindSimilar,
}: {
  suggestion: RuleSuggestion;
  onApply?: (s: RuleSuggestion) => void;
  onEdit?: (s: RuleSuggestion) => void;
  onFindSimilar?: (s: RuleSuggestion) => void;
}) {
  return (
    <div className="mt-2 flex flex-wrap gap-2">
      <button
        type="button"
        onClick={() => onApply?.(suggestion)}
        className="flex items-center gap-1.5 rounded-md bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-indigo-700"
      >
        <Play className="h-3 w-3" aria-hidden="true" />
        Apply Rule
      </button>
      <button
        type="button"
        onClick={() => onEdit?.(suggestion)}
        className="flex items-center gap-1.5 rounded-md border border-gray-200 px-3 py-1.5 text-xs font-medium text-gray-600 transition-colors hover:bg-gray-50 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
      >
        <Pencil className="h-3 w-3" aria-hidden="true" />
        Edit First
      </button>
      <button
        type="button"
        onClick={() => onFindSimilar?.(suggestion)}
        className="flex items-center gap-1.5 rounded-md border border-gray-200 px-3 py-1.5 text-xs font-medium text-gray-600 transition-colors hover:bg-gray-50 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
      >
        <Search className="h-3 w-3" aria-hidden="true" />
        Find Similar
      </button>
    </div>
  );
}

/**
 * Renders a single chat message bubble with support for code blocks,
 * rule suggestions, and action buttons.
 */
export function ChatMessage({ message, onApplyRule, onEditRule, onFindSimilar }: ChatMessageProps) {
  const isUser = message.role === 'user';
  const segments = parseContent(message.content);

  return (
    <div className={`flex gap-3 ${isUser ? 'flex-row-reverse' : 'flex-row'}`} role="listitem">
      {/* Avatar */}
      <div
        className={`flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full ${
          isUser
            ? 'bg-indigo-100 text-indigo-600 dark:bg-indigo-900/40 dark:text-indigo-300'
            : 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-300'
        }`}
        aria-hidden="true"
      >
        {isUser ? <User className="h-4 w-4" /> : <Bot className="h-4 w-4" />}
      </div>

      {/* Message body */}
      <div className={`max-w-[75%] ${isUser ? 'items-end' : 'items-start'}`}>
        <div
          className={`rounded-lg px-4 py-2.5 text-sm ${
            isUser
              ? 'bg-indigo-600 text-white'
              : 'bg-gray-100 text-gray-900 dark:bg-gray-800 dark:text-gray-100'
          }`}
        >
          {segments.map((segment, idx) =>
            segment.type === 'code' ? (
              <pre
                key={idx}
                className={`my-2 overflow-x-auto rounded-md p-3 font-mono text-xs ${
                  isUser
                    ? 'bg-indigo-700/50 text-indigo-100'
                    : 'bg-gray-200 text-gray-800 dark:bg-gray-900 dark:text-gray-200'
                }`}
              >
                <code>{segment.value}</code>
              </pre>
            ) : (
              <span key={idx} className="whitespace-pre-wrap">
                {segment.value}
              </span>
            ),
          )}
        </div>

        {/* Rule suggestion actions */}
        {!isUser &&
          message.suggestions?.map((suggestion, idx) => (
            <SuggestionActions
              key={idx}
              suggestion={suggestion}
              onApply={onApplyRule}
              onEdit={onEditRule}
              onFindSimilar={onFindSimilar}
            />
          ))}

        {/* Timestamp */}
        <p
          className={`mt-1 text-xs text-gray-400 dark:text-gray-500 ${
            isUser ? 'text-right' : 'text-left'
          }`}
        >
          {formatTime(message.timestamp)}
        </p>
      </div>
    </div>
  );
}
