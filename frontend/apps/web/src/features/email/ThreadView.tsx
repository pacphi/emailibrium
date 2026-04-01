import { useState } from 'react';
import { ArrowLeft, Loader2, ChevronDown, ChevronRight } from 'lucide-react';
import type { EmailThread } from '@emailibrium/types';
import { MessageBubble } from './MessageBubble';
import { EmailActions, type EmailViewContext } from './EmailActions';
import { ReplyBox } from './ReplyBox';

interface ThreadViewProps {
  thread: EmailThread | undefined;
  isLoading: boolean;
  isError: boolean;
  viewContext?: EmailViewContext;
  onBack: () => void;
  onArchive: () => void;
  onStar: () => void;
  onDelete: () => void;
  onReclassify: (category: string) => void;
  onMove: (groupId: string) => void;
  onSendReply: (body: string) => void;
  onSendForward: (to: string, body: string) => void;
  isSendingReply: boolean;
  onSpam?: () => void;
  onRestore?: () => void;
  onPermanentDelete?: () => void;
}

export function ThreadView({
  thread,
  isLoading,
  isError,
  viewContext = 'inbox',
  onBack,
  onArchive,
  onStar,
  onDelete,
  onReclassify,
  onMove,
  onSendReply,
  onSendForward,
  isSendingReply,
  onSpam,
  onRestore,
  onPermanentDelete,
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
  const groupedLinks = extractAndGroupLinks(thread);

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
        viewContext={viewContext}
        onArchive={onArchive}
        onStar={onStar}
        onDelete={onDelete}
        onReclassify={onReclassify}
        onMove={onMove}
        onSpam={onSpam}
        onRestore={onRestore}
        onPermanentDelete={onPermanentDelete}
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

        {/* Extracted links grouped by type */}
        {Object.keys(groupedLinks).length > 0 && <ThreadLinksSection groups={groupedLinks} />}
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

// ---------------------------------------------------------------------------
// Link extraction, deduplication, and grouping
// ---------------------------------------------------------------------------

type LinkType = 'action' | 'social' | 'document' | 'unsubscribe' | 'tracking' | 'other';

interface ExtractedLink {
  url: string;
  label: string;
  type: LinkType;
}

const LINK_TYPE_META: Record<LinkType, { title: string; order: number }> = {
  action: { title: 'Actions', order: 0 },
  document: { title: 'Documents & Pages', order: 1 },
  social: { title: 'Social & Reviews', order: 2 },
  unsubscribe: { title: 'Unsubscribe', order: 3 },
  other: { title: 'Other', order: 4 },
  tracking: { title: 'Tracking & Redirects', order: 5 },
};

function classifyLink(url: string, label: string): LinkType {
  const lowerUrl = url.toLowerCase();
  const lowerLabel = label.toLowerCase();

  // Check both URL and anchor-text label for unsubscribe intent.
  if (/unsubscri/i.test(lowerUrl) || /unsubscri/i.test(lowerLabel)) return 'unsubscribe';

  // Label-based overrides: the anchor text is more meaningful than tracking URLs.
  if (/survey|form|feedback/i.test(lowerLabel)) return 'action';
  if (/tripadvisor|yelp|trustpilot|google/i.test(lowerLabel)) return 'social';
  if (/calendar|zoom|meet|webinar/i.test(lowerLabel)) return 'action';

  // Tracking / redirect URLs: long encoded paths, email tracking pixels, etc.
  if (
    /\/c\/eJx/i.test(lowerUrl) ||
    /\/track\//i.test(lowerUrl) ||
    /\/o\/eJx/i.test(lowerUrl) ||
    /click\.\w+\.\w+\//i.test(lowerUrl) ||
    /email\.mail\.\w+.*\/c\//i.test(lowerUrl) ||
    /\/wf\/click/i.test(lowerUrl) ||
    /list-manage\.com/i.test(lowerUrl) ||
    /\/e\/c\//i.test(lowerUrl)
  )
    return 'tracking';

  if (
    /tripadvisor|yelp|google\.com\/maps|trustpilot|facebook|twitter|x\.com|linkedin|instagram|youtube|reddit/i.test(
      lowerUrl,
    )
  )
    return 'social';

  if (/\.pdf($|\?)|docs\.google|drive\.google|dropbox|notion\.so|confluence/i.test(lowerUrl))
    return 'document';

  if (/survey|form|feedback|typeform|jotform|calendly|meetingbird|zoom\.us/i.test(lowerUrl))
    return 'action';

  return 'other';
}

/** Try to extract a human-readable label from an <a> tag in HTML for a given URL. */
function extractLabelFromHtml(html: string, url: string): string | null {
  // Escape special regex chars in URL for matching.
  const escaped = url.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const pattern = new RegExp(`<a[^>]*href=["']${escaped}["'][^>]*>([^<]+)<`, 'i');
  const match = html.match(pattern);
  if (match) {
    const text = match[1]!.trim();
    if (text && text.length > 1 && text.length < 120) return text;
  }
  return null;
}

/** Derive a short display label from a URL when no anchor text is available. */
function deriveLabel(url: string): string {
  try {
    const u = new URL(url);
    const host = u.hostname.replace(/^(www|email\.mail|mail|click)\./, '');
    const path = u.pathname.replace(/\/$/, '');
    if (path && path !== '/' && path.length < 60) {
      return `${host}${path}`;
    }
    return host;
  } catch {
    return url.length > 60 ? url.slice(0, 60) + '...' : url;
  }
}

function extractAndGroupLinks(thread: EmailThread): Record<LinkType, ExtractedLink[]> {
  const urlRegex = /https?:\/\/[^\s<>"']+/g;
  const seen = new Set<string>();
  const links: ExtractedLink[] = [];

  // Combine all HTML for label extraction.
  const allHtml = thread.emails.map((e) => e.bodyHtml ?? '').join(' ');

  for (const email of thread.emails) {
    // Extract from HTML only — bodyText duplicates the same links.
    const source = email.bodyHtml ?? email.bodyText ?? '';
    const matches = source.match(urlRegex);
    if (!matches) continue;

    for (const raw of matches) {
      // Normalize: strip trailing punctuation artifacts.
      const url = raw.replace(/[);,.']+$/, '');
      if (seen.has(url)) continue;
      seen.add(url);

      const anchorLabel = extractLabelFromHtml(allHtml, url);
      const label = anchorLabel ?? deriveLabel(url);
      const type = classifyLink(url, label);

      links.push({ url, label, type });
    }
  }

  // Group by type, deduplicating by label within each group.
  const groups: Record<string, ExtractedLink[]> = {};
  for (const link of links) {
    const bucket = (groups[link.type] ??= []);
    // Skip if we already have a link with the same display label in this group.
    if (!bucket.some((existing) => existing.label === link.label)) {
      bucket.push(link);
    }
  }

  // Return sorted by type order.
  const sorted: Record<LinkType, ExtractedLink[]> = {} as Record<LinkType, ExtractedLink[]>;
  const typeOrder = Object.entries(LINK_TYPE_META)
    .sort(([, a], [, b]) => a.order - b.order)
    .map(([key]) => key as LinkType);

  for (const type of typeOrder) {
    if (groups[type] && groups[type].length > 0) {
      sorted[type] = groups[type];
    }
  }

  return sorted;
}

// ---------------------------------------------------------------------------
// Thread links collapsible section
// ---------------------------------------------------------------------------

function ThreadLinksSection({ groups }: { groups: Record<LinkType, ExtractedLink[]> }) {
  const [isOpen, setIsOpen] = useState(false);
  const totalCount = Object.values(groups).reduce((n, g) => n + g.length, 0);

  return (
    <div className="rounded-lg border border-gray-200 bg-gray-50 dark:border-gray-700 dark:bg-gray-800/50">
      <button
        type="button"
        onClick={() => setIsOpen((prev) => !prev)}
        className="flex w-full items-center gap-2 px-4 py-2.5 text-left text-xs font-semibold uppercase tracking-wider text-gray-500 hover:bg-gray-100 dark:text-gray-400 dark:hover:bg-gray-700/50"
      >
        {isOpen ? (
          <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5" aria-hidden="true" />
        )}
        Links in this thread
        <span className="ml-auto text-[10px] font-medium normal-case tracking-normal text-gray-400 dark:text-gray-500">
          {totalCount} link{totalCount !== 1 ? 's' : ''}
        </span>
      </button>

      {isOpen && (
        <div className="space-y-3 px-4 pb-3">
          {(Object.entries(groups) as [LinkType, ExtractedLink[]][]).map(([type, links]) => (
            <div key={type}>
              <h4 className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-gray-400 dark:text-gray-500">
                {LINK_TYPE_META[type].title}
              </h4>
              <ul className="space-y-0.5">
                {links.map((link, i) => (
                  <li key={i}>
                    <a
                      href={link.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-sm text-indigo-600 hover:underline dark:text-indigo-400"
                      title={link.url}
                    >
                      {link.label}
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
