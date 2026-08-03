import { useEffect, useRef, useState } from 'react';
import { Send, Reply, ReplyAll, Forward } from 'lucide-react';
import type { Email } from '@emailibrium/types';
import type { ReplyOpenSignal } from './hooks/useEmailShortcuts';

type ReplyMode = 'reply' | 'reply-all' | 'forward';

interface ReplyBoxProps {
  originalEmail: Email;
  onSendReply: (body: string) => void;
  onSendForward: (to: string, body: string) => void;
  isSending: boolean;
  /** Set (a fresh object) to expand the box in the given mode -- e.g. from the `r`/`f`
   * keyboard shortcuts. Call `onOpenSignalConsumed` once applied so the caller can clear
   * it, mirroring the scrollToEmailId/onScrollToComplete pattern used elsewhere. */
  openSignal?: ReplyOpenSignal | null;
  onOpenSignalConsumed?: () => void;
}

export function ReplyBox({
  originalEmail,
  onSendReply,
  onSendForward,
  isSending,
  openSignal,
  onOpenSignalConsumed,
}: ReplyBoxProps) {
  const [mode, setMode] = useState<ReplyMode>('reply');
  const [body, setBody] = useState('');
  const [forwardTo, setForwardTo] = useState('');
  const [isExpanded, setIsExpanded] = useState(false);
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  const forwardToRef = useRef<HTMLInputElement>(null);

  // Reset to the pristine, collapsed state whenever the underlying email changes -- this
  // component instance is reused across different emails/threads (no key, and a warm
  // TanStack Query cache means it often doesn't unmount between selections), so without
  // this a draft typed for one email would silently carry over and get sent against a
  // different one. Declared before the openSignal effect below so, if both ever fire in
  // the same commit, the reset always applies first.
  useEffect(() => {
    setMode('reply');
    setBody('');
    setForwardTo('');
    setIsExpanded(false);
  }, [originalEmail.id]);

  useEffect(() => {
    if (!openSignal) return;
    setMode(openSignal.mode);
    setIsExpanded(true);
    onOpenSignalConsumed?.();
  }, [openSignal, onOpenSignalConsumed]);

  // Move focus into the editor whenever it expands or switches into forward mode: the
  // r/f shortcuts (and the "Click to reply..." button) open the box, and without focus
  // the very next keystroke would hit the page-global shortcuts instead of the draft.
  // Focusing also engages useKeyboard's editable-field guard, which is what suppresses
  // c/r/f/e/# while the user types.
  useEffect(() => {
    if (!isExpanded) return;
    if (mode === 'forward') {
      forwardToRef.current?.focus();
    } else {
      bodyRef.current?.focus();
    }
  }, [isExpanded, mode]);

  const quotedText = `\n\n---\nOn ${originalEmail.receivedAt}, ${originalEmail.fromName || originalEmail.fromAddr} wrote:\n> ${(
    originalEmail.bodyText ?? ''
  ).replace(/\n/g, '\n> ')}`;

  function handleSend() {
    if (mode === 'forward') {
      if (!forwardTo.trim()) return;
      onSendForward(forwardTo.trim(), body + quotedText);
    } else {
      onSendReply(body + quotedText);
    }
    setBody('');
    setForwardTo('');
    setIsExpanded(false);
  }

  function handleKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      handleSend();
    }
  }

  if (!isExpanded) {
    return (
      <div className="border-t border-gray-200 p-3 dark:border-gray-700">
        <button
          type="button"
          onClick={() => setIsExpanded(true)}
          className="w-full rounded-md border border-gray-200 bg-gray-50 px-4 py-2.5 text-left text-sm text-gray-400 transition-colors hover:border-gray-300 hover:bg-white dark:border-gray-600 dark:bg-gray-700 dark:text-gray-500 dark:hover:bg-gray-600"
        >
          Click to reply...
        </button>
      </div>
    );
  }

  return (
    <div className="border-t border-gray-200 bg-white p-3 dark:border-gray-700 dark:bg-gray-800">
      {/* Mode toggle */}
      <div className="mb-2 flex items-center gap-1">
        <ModeButton mode="reply" currentMode={mode} icon={Reply} label="Reply" onSelect={setMode} />
        <ModeButton
          mode="reply-all"
          currentMode={mode}
          icon={ReplyAll}
          label="Reply All"
          onSelect={setMode}
        />
        <ModeButton
          mode="forward"
          currentMode={mode}
          icon={Forward}
          label="Forward"
          onSelect={setMode}
        />
      </div>

      {/* Recipients */}
      <div className="mb-2 space-y-1 text-sm">
        {mode === 'forward' ? (
          <div className="flex items-center gap-2">
            <label htmlFor="forward-to" className="text-gray-500 dark:text-gray-400">
              To:
            </label>
            <input
              id="forward-to"
              ref={forwardToRef}
              type="email"
              value={forwardTo}
              onChange={(e) => setForwardTo(e.target.value)}
              placeholder="recipient@example.com"
              className="flex-1 rounded border border-gray-200 bg-transparent px-2 py-1 text-sm text-gray-900 outline-none focus:border-indigo-400 dark:border-gray-600 dark:text-white"
            />
          </div>
        ) : (
          <p className="text-gray-500 dark:text-gray-400">
            To: {mode === 'reply' ? originalEmail.fromAddr : originalEmail.toAddrs}
            {mode === 'reply-all' && originalEmail.ccAddrs && (
              <span> | Cc: {originalEmail.ccAddrs}</span>
            )}
          </p>
        )}
      </div>

      {/* Body */}
      <textarea
        ref={bodyRef}
        value={body}
        onChange={(e) => setBody(e.target.value)}
        onKeyDown={handleKeyDown}
        rows={5}
        placeholder="Type your message..."
        className="w-full resize-y rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 outline-none focus:border-indigo-400 focus:ring-1 focus:ring-indigo-400 dark:border-gray-600 dark:bg-gray-700 dark:text-white"
        aria-label="Reply message body"
      />

      {/* Actions */}
      <div className="mt-2 flex items-center justify-between">
        <button
          type="button"
          onClick={() => {
            setIsExpanded(false);
            setBody('');
          }}
          className="text-sm text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
        >
          Discard
        </button>
        <button
          type="button"
          onClick={handleSend}
          disabled={isSending || (!body.trim() && mode !== 'forward')}
          className="flex items-center gap-1.5 rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700 disabled:cursor-not-allowed disabled:opacity-50"
          aria-label="Send reply"
        >
          <Send className="h-4 w-4" aria-hidden="true" />
          Send
        </button>
      </div>

      <p className="mt-1 text-right text-xs text-gray-400 dark:text-gray-500">Ctrl+Enter to send</p>
    </div>
  );
}

function ModeButton({
  mode,
  currentMode,
  icon: Icon,
  label,
  onSelect,
}: {
  mode: ReplyMode;
  currentMode: ReplyMode;
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  onSelect: (mode: ReplyMode) => void;
}) {
  const isActive = mode === currentMode;
  return (
    <button
      type="button"
      onClick={() => onSelect(mode)}
      className={`flex items-center gap-1 rounded-md px-2.5 py-1 text-xs font-medium transition-colors ${
        isActive
          ? 'bg-indigo-100 text-indigo-700 dark:bg-indigo-900/50 dark:text-indigo-200'
          : 'text-gray-500 hover:bg-gray-100 dark:text-gray-400 dark:hover:bg-gray-700'
      }`}
      aria-pressed={isActive}
    >
      <Icon className="h-3.5 w-3.5" aria-hidden="true" />
      {label}
    </button>
  );
}
