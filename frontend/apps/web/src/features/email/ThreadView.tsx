import { ArrowLeft, Loader2 } from 'lucide-react';
import type { EmailThread } from '@emailibrium/types';
import { MessageBubble } from './MessageBubble';
import { EmailActions } from './EmailActions';
import { ReplyBox } from './ReplyBox';

interface ThreadViewProps {
  thread: EmailThread | undefined;
  isLoading: boolean;
  isError: boolean;
  onBack: () => void;
  onArchive: () => void;
  onStar: () => void;
  onDelete: () => void;
  onReclassify: (category: string) => void;
  onMove: (groupId: string) => void;
  onSendReply: (body: string) => void;
  onSendForward: (to: string, body: string) => void;
  isSendingReply: boolean;
}

export function ThreadView({
  thread,
  isLoading,
  isError,
  onBack,
  onArchive,
  onStar,
  onDelete,
  onReclassify,
  onMove,
  onSendReply,
  onSendForward,
  isSendingReply,
}: ThreadViewProps) {
  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-6 w-6 animate-spin text-indigo-500" />
        <span className="ml-2 text-sm text-gray-500">Loading thread...</span>
      </div>
    );
  }

  if (isError) {
    return (
      <div className="flex h-full items-center justify-center">
        <p className="text-sm text-red-500">Failed to load thread.</p>
      </div>
    );
  }

  if (!thread) {
    return (
      <div className="flex h-full flex-col items-center justify-center text-gray-400 dark:text-gray-500">
        <p className="text-lg">Select an email to view</p>
        <p className="mt-1 text-sm">Choose from the list on the left</p>
      </div>
    );
  }

  const lastEmail = thread.emails[thread.emails.length - 1];
  const links = extractLinks(thread);

  return (
    <div className="flex h-full flex-col">
      {/* Thread header */}
      <div className="border-b border-gray-200 bg-white px-4 py-3 dark:border-gray-700 dark:bg-gray-800">
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={onBack}
            className="rounded-md p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300 lg:hidden"
            aria-label="Back to list"
          >
            <ArrowLeft className="h-5 w-5" />
          </button>
          <h2 className="flex-1 truncate text-lg font-semibold text-gray-900 dark:text-white">
            {thread.subject}
          </h2>
          <span className="shrink-0 text-xs text-gray-400 dark:text-gray-500">
            {thread.emails.length} message{thread.emails.length !== 1 ? 's' : ''}
          </span>
        </div>
      </div>

      {/* Actions bar */}
      <EmailActions
        emailId={thread.threadId}
        selectedCount={0}
        onArchive={onArchive}
        onStar={onStar}
        onDelete={onDelete}
        onReclassify={onReclassify}
        onMove={onMove}
      />

      {/* Messages */}
      <div className="flex-1 space-y-3 overflow-y-auto p-4">
        {thread.emails.map((email, index) => (
          <MessageBubble
            key={email.id}
            email={email}
            isLatest={index === thread.emails.length - 1}
          />
        ))}

        {/* Extracted links */}
        {links.length > 0 && (
          <div className="rounded-lg border border-gray-200 bg-gray-50 p-4 dark:border-gray-700 dark:bg-gray-800/50">
            <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">
              Links in this thread
            </h3>
            <ul className="space-y-1">
              {links.map((link, i) => (
                <li key={i}>
                  <a
                    href={link}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-sm text-indigo-600 hover:underline dark:text-indigo-400"
                  >
                    {link.length > 80 ? link.slice(0, 80) + '...' : link}
                  </a>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>

      {/* Reply box */}
      {lastEmail && (
        <ReplyBox
          originalEmail={lastEmail}
          onSendReply={onSendReply}
          onSendForward={onSendForward}
          isSending={isSendingReply}
        />
      )}
    </div>
  );
}

function extractLinks(thread: EmailThread): string[] {
  const urlRegex = /https?:\/\/[^\s<>"']+/g;
  const allLinks = new Set<string>();
  for (const email of thread.emails) {
    const text = (email.bodyText ?? '') + ' ' + (email.bodyHtml ?? '');
    const matches = text.match(urlRegex);
    if (matches) {
      for (const url of matches) {
        allLinks.add(url);
      }
    }
  }
  return Array.from(allLinks).slice(0, 20);
}
