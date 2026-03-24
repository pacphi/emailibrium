import { useState, useCallback, useMemo } from 'react';
import { Plus } from 'lucide-react';
import { EmailSidebar } from './EmailSidebar';
import type { SidebarGroup } from './EmailSidebar';
import { EmailList } from './EmailList';
import { ThreadView } from './ThreadView';
import { ComposeEmail } from './ComposeEmail';
import {
  useEmailsQuery,
  useThreadQuery,
  useArchiveEmail,
  useStarEmail,
  useDeleteEmail,
  useReplyToEmail,
  useForwardEmail,
  useBulkArchive,
  useBulkDelete,
} from './hooks/useEmails';

// Default sidebar groups -- in production these would come from an API or store.
const defaultGroups: SidebarGroup[] = [
  { id: 'inbox', label: 'Inbox', icon: 'inbox', unreadCount: 0 },
  { id: 'cat-personal', label: 'Personal', icon: 'category', unreadCount: 0 },
  { id: 'cat-work', label: 'Work', icon: 'category', unreadCount: 0 },
  { id: 'cat-finance', label: 'Finance', icon: 'category', unreadCount: 0 },
  { id: 'cat-shopping', label: 'Shopping', icon: 'category', unreadCount: 0 },
  { id: 'topic-projects', label: 'Projects', icon: 'topic', unreadCount: 0 },
  { id: 'topic-travel', label: 'Travel', icon: 'topic', unreadCount: 0 },
  { id: 'sub-newsletters', label: 'Newsletters', icon: 'subscription', unreadCount: 0 },
  { id: 'sub-marketing', label: 'Marketing', icon: 'subscription', unreadCount: 0 },
];

function groupToQueryParam(groupId: string): { category?: string } {
  if (groupId === 'inbox') return {};
  if (groupId.startsWith('cat-')) return { category: groupId.replace('cat-', '') };
  if (groupId.startsWith('topic-')) return { category: groupId.replace('topic-', '') };
  if (groupId.startsWith('sub-')) return { category: groupId.replace('sub-', '') };
  return {};
}

export function EmailClient() {
  const [activeGroup, setActiveGroup] = useState('inbox');
  const [selectedEmailId, setSelectedEmailId] = useState<string | null>(null);
  const [checkedIds, setCheckedIds] = useState<Set<string>>(new Set());
  const [isComposeOpen, setIsComposeOpen] = useState(false);
  const [mobilePanel, setMobilePanel] = useState<'sidebar' | 'list' | 'thread'>('list');

  const queryParams = useMemo(
    () => ({ ...groupToQueryParam(activeGroup), limit: 50 }),
    [activeGroup],
  );
  const emailsQuery = useEmailsQuery(queryParams);
  const emails = emailsQuery.data?.emails ?? [];

  const selectedEmail = emails.find((e) => e.id === selectedEmailId) ?? null;
  const threadId = selectedEmail?.threadId ?? null;
  const threadQuery = useThreadQuery(threadId);

  const archiveMutation = useArchiveEmail();
  const starMutation = useStarEmail();
  const deleteMutation = useDeleteEmail();
  const replyMutation = useReplyToEmail();
  const forwardMutation = useForwardEmail();
  const bulkArchiveMutation = useBulkArchive();
  const bulkDeleteMutation = useBulkDelete();

  // Compute unread counts per group
  const groupsWithCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const email of emails) {
      if (!email.isRead) {
        counts['inbox'] = (counts['inbox'] ?? 0) + 1;
        if (email.category) {
          const key = `cat-${email.category.toLowerCase()}`;
          counts[key] = (counts[key] ?? 0) + 1;
        }
      }
    }
    return defaultGroups.map((g) => ({
      ...g,
      unreadCount: counts[g.id] ?? 0,
    }));
  }, [emails]);

  const handleGroupSelect = useCallback((groupId: string) => {
    setActiveGroup(groupId);
    setSelectedEmailId(null);
    setCheckedIds(new Set());
    setMobilePanel('list');
  }, []);

  const handleSelectEmail = useCallback((emailId: string) => {
    setSelectedEmailId(emailId);
    setMobilePanel('thread');
  }, []);

  const handleCheckEmail = useCallback((emailId: string, checked: boolean) => {
    setCheckedIds((prev) => {
      const next = new Set(prev);
      if (checked) next.add(emailId);
      else next.delete(emailId);
      return next;
    });
  }, []);

  const handleStarEmail = useCallback(
    (emailId: string) => {
      starMutation.mutate(emailId);
    },
    [starMutation],
  );

  const handleArchiveEmail = useCallback(
    (emailId: string) => {
      archiveMutation.mutate(emailId);
      if (selectedEmailId === emailId) setSelectedEmailId(null);
    },
    [archiveMutation, selectedEmailId],
  );

  const handleDeleteEmailFromList = useCallback(
    (emailId: string) => {
      deleteMutation.mutate(emailId);
      if (selectedEmailId === emailId) setSelectedEmailId(null);
    },
    [deleteMutation, selectedEmailId],
  );

  // Thread-level actions
  const handleThreadArchive = useCallback(() => {
    if (checkedIds.size > 0) {
      bulkArchiveMutation.mutate(Array.from(checkedIds));
      setCheckedIds(new Set());
    } else if (selectedEmailId) {
      archiveMutation.mutate(selectedEmailId);
    }
  }, [checkedIds, selectedEmailId, bulkArchiveMutation, archiveMutation]);

  const handleThreadStar = useCallback(() => {
    if (selectedEmailId) starMutation.mutate(selectedEmailId);
  }, [selectedEmailId, starMutation]);

  const handleThreadDelete = useCallback(() => {
    if (checkedIds.size > 0) {
      bulkDeleteMutation.mutate(Array.from(checkedIds));
      setCheckedIds(new Set());
    } else if (selectedEmailId) {
      deleteMutation.mutate(selectedEmailId);
      setSelectedEmailId(null);
    }
  }, [checkedIds, selectedEmailId, bulkDeleteMutation, deleteMutation]);

  const handleReclassify = useCallback((_category: string) => {
    // Reclassify triggers learning feedback via API -- placeholder for the endpoint
  }, []);

  const handleMove = useCallback((_groupId: string) => {
    // Move to group -- placeholder
  }, []);

  const handleSendReply = useCallback(
    (body: string) => {
      if (!selectedEmailId) return;
      replyMutation.mutate({ id: selectedEmailId, body: { bodyText: body } });
    },
    [selectedEmailId, replyMutation],
  );

  const handleSendForward = useCallback(
    (to: string, _body: string) => {
      if (!selectedEmailId) return;
      forwardMutation.mutate({ id: selectedEmailId, to });
    },
    [selectedEmailId, forwardMutation],
  );

  const handleBackToList = useCallback(() => {
    setSelectedEmailId(null);
    setMobilePanel('list');
  }, []);

  // Stub accounts list -- would come from auth store
  const accounts = useMemo(
    () => [{ id: 'acc-1', emailAddress: 'user@gmail.com', provider: 'gmail' }],
    [],
  );

  return (
    <div className="flex h-full">
      {/* Left sidebar -- hidden on mobile when not active */}
      <div className={`${mobilePanel === 'sidebar' ? 'flex' : 'hidden'} lg:flex`}>
        <EmailSidebar
          groups={groupsWithCounts}
          activeGroupId={activeGroup}
          onGroupSelect={handleGroupSelect}
        />
      </div>

      {/* Middle panel: email list */}
      <div
        className={`${
          mobilePanel === 'list' ? 'flex' : 'hidden'
        } w-full flex-col border-r border-gray-200 dark:border-gray-700 lg:flex lg:w-96`}
      >
        {/* List header */}
        <div className="flex items-center justify-between border-b border-gray-200 bg-white px-3 py-2 dark:border-gray-700 dark:bg-gray-800">
          <button
            type="button"
            onClick={() => setMobilePanel('sidebar')}
            className="text-sm text-gray-500 hover:text-gray-700 dark:text-gray-400 lg:hidden"
            aria-label="Show sidebar"
          >
            Groups
          </button>
          <h2 className="text-sm font-semibold text-gray-900 dark:text-white">
            {defaultGroups.find((g) => g.id === activeGroup)?.label ?? 'Inbox'}
          </h2>
          <button
            type="button"
            onClick={() => setIsComposeOpen(true)}
            className="flex items-center gap-1 rounded-md bg-indigo-600 px-2.5 py-1 text-xs font-medium text-white transition-colors hover:bg-indigo-700"
            aria-label="Compose new email"
          >
            <Plus className="h-3.5 w-3.5" aria-hidden="true" />
            Compose
          </button>
        </div>
        <EmailList
          emails={emails}
          selectedEmailId={selectedEmailId}
          checkedEmailIds={checkedIds}
          isLoading={emailsQuery.isLoading}
          isError={emailsQuery.isError}
          onSelectEmail={handleSelectEmail}
          onCheckEmail={handleCheckEmail}
          onStarEmail={handleStarEmail}
          onArchiveEmail={handleArchiveEmail}
          onDeleteEmail={handleDeleteEmailFromList}
        />
      </div>

      {/* Right panel: thread view */}
      <div
        className={`${
          mobilePanel === 'thread' ? 'flex' : 'hidden'
        } min-w-0 flex-1 flex-col lg:flex`}
      >
        <ThreadView
          thread={threadQuery.data}
          isLoading={threadQuery.isLoading}
          isError={threadQuery.isError}
          onBack={handleBackToList}
          onArchive={handleThreadArchive}
          onStar={handleThreadStar}
          onDelete={handleThreadDelete}
          onReclassify={handleReclassify}
          onMove={handleMove}
          onSendReply={handleSendReply}
          onSendForward={handleSendForward}
          isSendingReply={replyMutation.isPending || forwardMutation.isPending}
        />
      </div>

      {/* Compose modal */}
      <ComposeEmail
        isOpen={isComposeOpen}
        onClose={() => setIsComposeOpen(false)}
        accounts={accounts}
      />
    </div>
  );
}
