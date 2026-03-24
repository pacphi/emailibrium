import { useState, useRef, useCallback, type KeyboardEvent } from 'react';
import { ArrowUp } from 'lucide-react';

interface ChatInputProps {
  onSend: (message: string) => void;
  disabled?: boolean;
  maxLength?: number;
}

const DEFAULT_MAX_LENGTH = 2000;

/**
 * Chat message input bar with send button, keyboard shortcuts,
 * and character count indicator.
 */
export function ChatInput({
  onSend,
  disabled = false,
  maxLength = DEFAULT_MAX_LENGTH,
}: ChatInputProps) {
  const [value, setValue] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const handleSend = useCallback(() => {
    const trimmed = value.trim();
    if (!trimmed || disabled) return;

    onSend(trimmed);
    setValue('');

    // Reset textarea height after sending
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
  }, [value, disabled, onSend]);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      }
    },
    [handleSend],
  );

  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLTextAreaElement>) => {
      const newValue = e.target.value;
      if (newValue.length <= maxLength) {
        setValue(newValue);
      }

      // Auto-resize textarea
      const textarea = e.target;
      textarea.style.height = 'auto';
      textarea.style.height = `${Math.min(textarea.scrollHeight, 160)}px`;
    },
    [maxLength],
  );

  const charCount = value.length;
  const isNearLimit = charCount > maxLength * 0.9;
  const canSend = value.trim().length > 0 && !disabled;

  return (
    <div className="border-t border-gray-200 bg-white px-4 py-3 dark:border-gray-700 dark:bg-gray-900">
      <div className="flex items-end gap-2">
        <div className="relative flex-1">
          <textarea
            ref={textareaRef}
            value={value}
            onChange={handleChange}
            onKeyDown={handleKeyDown}
            placeholder="Ask about your emails..."
            disabled={disabled}
            rows={1}
            className="w-full resize-none rounded-lg border border-gray-200 bg-gray-50 px-4 py-2.5 text-sm text-gray-900 placeholder-gray-400 outline-none transition-colors focus:border-indigo-400 focus:bg-white focus:ring-1 focus:ring-indigo-400 disabled:cursor-not-allowed disabled:opacity-50 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-100 dark:placeholder-gray-500 dark:focus:border-indigo-500 dark:focus:bg-gray-800"
            aria-label="Chat message input"
          />
        </div>
        <button
          type="button"
          onClick={handleSend}
          disabled={!canSend}
          className="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-lg bg-indigo-600 text-white transition-colors hover:bg-indigo-700 disabled:cursor-not-allowed disabled:opacity-40"
          aria-label="Send message"
        >
          <ArrowUp className="h-5 w-5" />
        </button>
      </div>

      {/* Character count */}
      <div className="mt-1 flex justify-between text-xs">
        <span className="text-gray-400 dark:text-gray-500">
          {disabled ? 'Waiting for response...' : 'Enter to send, Shift+Enter for newline'}
        </span>
        {charCount > 0 && (
          <span className={isNearLimit ? 'text-amber-500' : 'text-gray-400 dark:text-gray-500'}>
            {charCount}/{maxLength}
          </span>
        )}
      </div>
    </div>
  );
}
