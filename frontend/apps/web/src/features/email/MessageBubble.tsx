import { useState } from 'react';
import { ChevronDown, ChevronUp, Download, FileText } from 'lucide-react';
import { format } from 'date-fns';
import type { Email } from '@emailibrium/types';

interface MessageBubbleProps {
  email: Email;
  isLatest: boolean;
}

interface AttachmentStub {
  name: string;
  size: string;
}

function parseAttachmentStubs(_email: Email): AttachmentStub[] {
  // In a real app, attachments would come from the API; this is a placeholder.
  if (!_email.hasAttachments) return [];
  return [{ name: 'attachment.pdf', size: '124 KB' }];
}

function SanitizedHtml({ html }: { html: string }) {
  // Basic sanitisation -- a production build would use DOMPurify.
  const cleaned = html
    .replace(/<script[\s\S]*?<\/script>/gi, '')
    .replace(/on\w+="[^"]*"/gi, '')
    .replace(/javascript:/gi, '');

  return (
    <div
      className="prose prose-sm max-w-none dark:prose-invert"
      // eslint-disable-next-line react/no-danger
      dangerouslySetInnerHTML={{ __html: cleaned }}
    />
  );
}

export function MessageBubble({ email, isLatest }: MessageBubbleProps) {
  const [isExpanded, setIsExpanded] = useState(isLatest);
  const attachments = parseAttachmentStubs(email);
  const dateStr = format(new Date(email.receivedAt), 'MMM d, yyyy h:mm a');

  return (
    <article
      className="rounded-lg border border-gray-200 bg-white dark:border-gray-700 dark:bg-gray-800"
      aria-label={`Message from ${email.fromName || email.fromAddr}`}
    >
      {/* Header */}
      <button
        type="button"
        onClick={() => setIsExpanded((prev) => !prev)}
        className="flex w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-gray-50 dark:hover:bg-gray-700/50"
        aria-expanded={isExpanded}
      >
        <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-indigo-100 text-xs font-semibold text-indigo-700 dark:bg-indigo-900 dark:text-indigo-200">
          {(email.fromName || email.fromAddr || '?')[0]?.toUpperCase()}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <span className="truncate text-sm font-semibold text-gray-900 dark:text-white">
              {email.fromName || email.fromAddr}
            </span>
            <span className="text-xs text-gray-400 dark:text-gray-500">
              {'<'}
              {email.fromAddr}
              {'>'}
            </span>
          </div>
          {!isExpanded && (
            <p className="truncate text-xs text-gray-500 dark:text-gray-400">
              {email.bodyText?.slice(0, 100)}
            </p>
          )}
        </div>
        <span className="shrink-0 text-xs text-gray-400 dark:text-gray-500">{dateStr}</span>
        {isExpanded ? (
          <ChevronUp className="h-4 w-4 shrink-0 text-gray-400" aria-hidden="true" />
        ) : (
          <ChevronDown className="h-4 w-4 shrink-0 text-gray-400" aria-hidden="true" />
        )}
      </button>

      {/* Body */}
      {isExpanded && (
        <div className="border-t border-gray-100 px-4 py-3 dark:border-gray-700">
          <div className="mb-2 text-xs text-gray-400 dark:text-gray-500">
            To: {email.toAddrs}
            {email.ccAddrs && <span> | Cc: {email.ccAddrs}</span>}
          </div>

          {email.bodyHtml ? (
            <SanitizedHtml html={email.bodyHtml} />
          ) : (
            <p className="whitespace-pre-wrap text-sm text-gray-700 dark:text-gray-300">
              {email.bodyText || '(No content)'}
            </p>
          )}

          {/* Attachments */}
          {attachments.length > 0 && (
            <div className="mt-4 space-y-2">
              <h4 className="text-xs font-semibold uppercase text-gray-500 dark:text-gray-400">
                Attachments
              </h4>
              <div className="flex flex-wrap gap-2">
                {attachments.map((att) => (
                  <div
                    key={att.name}
                    className="flex items-center gap-2 rounded-md border border-gray-200 bg-gray-50 px-3 py-2 dark:border-gray-600 dark:bg-gray-700"
                  >
                    <FileText className="h-4 w-4 text-gray-400" aria-hidden="true" />
                    <div>
                      <p className="text-sm font-medium text-gray-700 dark:text-gray-200">
                        {att.name}
                      </p>
                      <p className="text-xs text-gray-400">{att.size}</p>
                    </div>
                    <button
                      type="button"
                      className="rounded p-1 text-gray-400 hover:text-indigo-600 dark:hover:text-indigo-400"
                      aria-label={`Download ${att.name}`}
                    >
                      <Download className="h-4 w-4" />
                    </button>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </article>
  );
}
