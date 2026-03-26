import { useState, useCallback, useMemo } from 'react';
import { Plus } from 'lucide-react';
import { submitFeedback } from '@emailibrium/api';
import { EmailSidebar } from './EmailSidebar';
import type { SidebarGroup } from './EmailSidebar';
import { EmailList } from './EmailList';
import { GroupedEmailList } from './GroupedEmailList';
import { groupByDomain } from './utils/groupBySender';
import { ThreadView } from './ThreadView';
import { ComposeEmail } from './ComposeEmail';
import { MoveDialog } from './MoveDialog';
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
  useCategoriesQuery,
  useLabelsQuery,
  useMoveEmail,
  useMarkRead,
} from './hooks/useEmails';

const SUBSCRIPTION_CATEGORIES = new Set([
  'newsletters', 'marketing', 'promotions', 'updates', 'digests',
]);
const TOPIC_CATEGORIES = new Set([
  'projects', 'travel', 'events', 'meetings',
]);

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
  const [filter, setFilter] = useState<'all' | 'read' | 'unread' | 'starred'>('all');
  const [isGrouped, setIsGrouped] = useState(false);
  const [expandedDomains, setExpandedDomains] = useState<Set<string>>(new Set());
  const [expandedSenders, setExpandedSenders] = useState<Set<string>>(new Set());
  const [searchText, setSearchText] = useState('');
  const [searchField, setSearchField] = useState<'from' | 'to' | 'cc' | 'subject' | 'body'>('from');

  const queryParams = useMemo(() => {
    const base = groupToQueryParam(activeGroup);
    if (filter === 'read') return { ...base, isRead: true };
    if (filter === 'unread') return { ...base, isRead: false };
    if (filter === 'starred') return { ...base, isStarred: true };
    return base;
  }, [activeGroup, filter]);
  const emailsQuery = useEmailsQuery(queryParams);
  const emails = useMemo(
    () => emailsQuery.data?.pages.flatMap((p) => p.emails) ?? [],
    [emailsQuery.data?.pages],
  );
  const totalEmails = emailsQuery.data?.pages[0]?.total ?? 0;

  const filteredEmails = useMemo(() => {
    if (!searchText.trim()) return emails;
    const needle = searchText.toLowerCase();
    return emails.filter((email) => {
      switch (searchField) {
        case 'from':
          return email.fromAddr.toLowerCase().includes(needle) ||
                 (email.fromName?.toLowerCase().includes(needle) ?? false);
        case 'to':
          return email.toAddrs.toLowerCase().includes(needle);
        case 'cc':
          return (email.ccAddrs?.toLowerCase().includes(needle) ?? false);
        case 'subject':
          return email.subject.toLowerCase().includes(needle);
        case 'body':
          return (email.bodyText?.toLowerCase().includes(needle) ?? false);
        default:
          return true;
      }
    });
  }, [emails, searchText, searchField]);

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
  const moveMutation = useMoveEmail();
  const markReadMutation = useMarkRead();

  // Determine account ID from the first email for label fetching.
  const currentAccountId = emails.length > 0 ? emails[0]?.accountId : undefined;
  const labelsQuery = useLabelsQuery(currentAccountId);

  // Fetch real categories from the database.
  const categoriesQuery = useCategoriesQuery();
  const categories = categoriesQuery.data?.categories ?? [];

  // Build sidebar groups dynamically from real category data.
  const sidebarGroups = useMemo(() => {
    const groups: SidebarGroup[] = [
      { id: 'inbox', label: 'Inbox', icon: 'inbox', unreadCount: 0 },
    ];

    for (const cat of categories) {
      const lower = cat.toLowerCase();
      if (SUBSCRIPTION_CATEGORIES.has(lower)) {
        groups.push({ id: `sub-${lower}`, label: cat, icon: 'subscription', unreadCount: 0 });
      } else if (TOPIC_CATEGORIES.has(lower)) {
        groups.push({ id: `topic-${lower}`, label: cat, icon: 'topic', unreadCount: 0 });
      } else {
        groups.push({ id: `cat-${lower}`, label: cat, icon: 'category', unreadCount: 0 });
      }
    }

    return groups;
  }, [categories]);

  // Compute counts per group. Inbox shows total email count.
  const groupsWithCounts = useMemo(() => {
    const counts: Record<string, number> = { inbox: totalEmails };
    for (const email of emails) {
      if (email.category && email.category !== 'Uncategorized') {
        const lower = email.category.toLowerCase();
        // Increment count for whichever prefix this category uses in sidebarGroups.
        for (const prefix of ['cat-', 'topic-', 'sub-']) {
          const key = `${prefix}${lower}`;
          if (sidebarGroups.some((g) => g.id === key)) {
            counts[key] = (counts[key] ?? 0) + 1;
            break;
          }
        }
      }
    }
    return sidebarGroups.map((g) => ({
      ...g,
      unreadCount: counts[g.id] ?? 0,
    }));
  }, [emails, sidebarGroups, totalEmails]);

  const domainGroups = useMemo(
    () => (isGrouped ? groupByDomain(filteredEmails) : []),
    [isGrouped, filteredEmails],
  );

  const toggleDomainExpand = useCallback((domain: string) => {
    setExpandedDomains((prev) => {
      const next = new Set(prev);
      if (next.has(domain)) next.delete(domain);
      else next.add(domain);
      return next;
    });
  }, []);

  const toggleSenderExpand = useCallback((groupKey: string) => {
    setExpandedSenders((prev) => {
      const next = new Set(prev);
      if (next.has(groupKey)) next.delete(groupKey);
      else next.add(groupKey);
      return next;
    });
  }, []);

  const handleToggleGrouped = useCallback(() => {
    setIsGrouped((prev) => !prev);
    setExpandedDomains(new Set());
    setExpandedSenders(new Set());
  }, []);

  const handleGroupSelect = useCallback((groupId: string) => {
    setActiveGroup(groupId);
    setSelectedEmailId(null);
    setCheckedIds(new Set());
    setMobilePanel('list');
  }, []);

  const handleSelectEmail = useCallback(
    (emailId: string) => {
      setSelectedEmailId(emailId);
      setMobilePanel('thread');
      // Auto-mark as read when selecting an unread email.
      const email = emails.find((e) => e.id === emailId);
      if (email && !email.isRead) {
        markReadMutation.mutate({ id: emailId, read: true });
      }
    },
    [emails, markReadMutation],
  );

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

  const handleGroupBulkArchive = useCallback(
    (emailIds: string[]) => {
      bulkArchiveMutation.mutate(emailIds);
      if (selectedEmailId && emailIds.includes(selectedEmailId)) {
        setSelectedEmailId(null);
      }
    },
    [bulkArchiveMutation, selectedEmailId],
  );

  const handleGroupBulkDelete = useCallback(
    (emailIds: string[]) => {
      bulkDeleteMutation.mutate(emailIds);
      if (selectedEmailId && emailIds.includes(selectedEmailId)) {
        setSelectedEmailId(null);
      }
    },
    [bulkDeleteMutation, selectedEmailId],
  );

  const handleGroupBulkMarkRead = useCallback(
    (emailIds: string[]) => {
      for (const id of emailIds) {
        markReadMutation.mutate({ id, read: true });
      }
    },
    [markReadMutation],
  );

  const handleGroupBulkMarkUnread = useCallback(
    (emailIds: string[]) => {
      for (const id of emailIds) {
        markReadMutation.mutate({ id, read: false });
      }
    },
    [markReadMutation],
  );

  // Move dialog state — supports single email or bulk.
  const [moveDialogEmailId, setMoveDialogEmailId] = useState<string | null>(null);
  const [bulkMoveIds, setBulkMoveIds] = useState<string[] | null>(null);
  const [bulkMoveSubject, setBulkMoveSubject] = useState('');
  const moveDialogEmail = moveDialogEmailId
    ? emails.find((e) => e.id === moveDialogEmailId)
    : null;
  const isMoveOpen = moveDialogEmailId !== null || bulkMoveIds !== null;

  const handleMoveOpen = useCallback((emailId: string) => {
    setMoveDialogEmailId(emailId);
  }, []);

  const handleBulkMoveOpen = useCallback((emailIds: string[], subject: string) => {
    setBulkMoveIds(emailIds);
    setBulkMoveSubject(subject);
  }, []);

  const handleMoveConfirm = useCallback(
    (targetId: string, kind: 'folder' | 'label') => {
      if (bulkMoveIds) {
        // Bulk move: move each email to the target
        for (const id of bulkMoveIds) {
          const email = emails.find((e) => e.id === id);
          if (email) {
            moveMutation.mutate({ id: email.id, accountId: email.accountId, targetId, kind });
          }
        }
        if (kind === 'folder' && selectedEmailId && bulkMoveIds.includes(selectedEmailId)) {
          setSelectedEmailId(null);
        }
        setBulkMoveIds(null);
        setBulkMoveSubject('');
      } else if (moveDialogEmail) {
        moveMutation.mutate({
          id: moveDialogEmail.id,
          accountId: moveDialogEmail.accountId,
          targetId,
          kind,
        });
        if (kind === 'folder' && selectedEmailId === moveDialogEmail.id) {
          setSelectedEmailId(null);
        }
        setMoveDialogEmailId(null);
      }
    },
    [bulkMoveIds, moveDialogEmail, moveMutation, selectedEmailId, emails],
  );

  const handleMoveClose = useCallback(() => {
    setMoveDialogEmailId(null);
    setBulkMoveIds(null);
    setBulkMoveSubject('');
  }, []);

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

  const handleReclassify = useCallback(
    async (category: string) => {
      if (!selectedEmailId || !selectedEmail) return;
      await submitFeedback({
        email_id: selectedEmailId,
        action: { type: 'reclassify', from: selectedEmail.category, to: category },
      });
      await emailsQuery.refetch();
    },
    [selectedEmailId, selectedEmail, emailsQuery],
  );

  const handleMove = useCallback(
    async (groupId: string) => {
      if (!selectedEmailId) return;
      await submitFeedback({
        email_id: selectedEmailId,
        action: { type: 'move_to_group', group_id: groupId },
      });
      await emailsQuery.refetch();
    },
    [selectedEmailId, emailsQuery],
  );

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

  const accounts = useMemo(
    () => [] as { id: string; emailAddress: string; provider: string }[],
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
        } w-full flex-col border-r border-gray-200 dark:border-gray-700 lg:flex lg:w-[600px]`}
      >
        {/* List header */}
        <div className="border-b border-gray-200 bg-white dark:border-gray-700 dark:bg-gray-800">
          <div className="flex items-center justify-between px-3 py-2">
            <button
              type="button"
              onClick={() => setMobilePanel('sidebar')}
              className="text-sm text-gray-500 hover:text-gray-700 dark:text-gray-400 lg:hidden"
              aria-label="Show sidebar"
            >
              Groups
            </button>
            <h2 className="text-sm font-semibold text-gray-900 dark:text-white">
              {sidebarGroups.find((g) => g.id === activeGroup)?.label ?? 'Inbox'}
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
          {/* Filter pills */}
          <div className="flex items-center gap-1 px-3 pb-2">
            {(['all', 'unread', 'read', 'starred'] as const).map((f) => (
              <button
                key={f}
                type="button"
                onClick={() => setFilter(f)}
                className={`rounded-full px-3 py-1 text-xs font-medium transition-colors ${
                  filter === f
                    ? 'bg-indigo-600 text-white'
                    : 'bg-gray-100 text-gray-600 hover:bg-gray-200 dark:bg-gray-700 dark:text-gray-300 dark:hover:bg-gray-600'
                }`}
              >
                {f.charAt(0).toUpperCase() + f.slice(1)}
              </button>
            ))}
            {/* Divider */}
            <div className="mx-1 h-4 w-px bg-gray-300 dark:bg-gray-600" />
            {/* Grouped toggle */}
            <button
              type="button"
              onClick={handleToggleGrouped}
              aria-pressed={isGrouped}
              className={`rounded-full px-3 py-1 text-xs font-medium transition-colors ${
                isGrouped
                  ? 'bg-indigo-600 text-white'
                  : 'bg-gray-100 text-gray-600 hover:bg-gray-200 dark:bg-gray-700 dark:text-gray-300 dark:hover:bg-gray-600'
              }`}
            >
              Grouped
            </button>
          </div>
          {/* Search filter bar */}
          <div className="flex items-center gap-2 px-3 pb-2">
            <select
              value={searchField}
              onChange={(e) => setSearchField(e.target.value as typeof searchField)}
              className="rounded-md border border-gray-300 bg-white px-2 py-1 text-xs text-gray-700 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-200"
              aria-label="Search field"
            >
              <option value="from">From</option>
              <option value="to">To</option>
              <option value="cc">CC</option>
              <option value="subject">Subject</option>
              <option value="body">Body</option>
            </select>
            <div className="relative flex-1">
              <input
                type="text"
                value={searchText}
                onChange={(e) => setSearchText(e.target.value)}
                placeholder={`Filter by ${searchField}...`}
                className="w-full rounded-md border border-gray-300 bg-white px-3 py-1 text-xs text-gray-700 placeholder-gray-400 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-200 dark:placeholder-gray-500"
                aria-label="Filter emails"
              />
              {searchText && (
                <button
                  type="button"
                  onClick={() => setSearchText('')}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
                  aria-label="Clear search"
                >
                  ×
                </button>
              )}
            </div>
          </div>
        </div>
        {isGrouped ? (
          <GroupedEmailList
            domains={domainGroups}
            expandedDomains={expandedDomains}
            expandedSenders={expandedSenders}
            onToggleDomain={toggleDomainExpand}
            onToggleSender={toggleSenderExpand}
            selectedEmailId={selectedEmailId}
            checkedEmailIds={checkedIds}
            onSelectEmail={handleSelectEmail}
            onCheckEmail={handleCheckEmail}
            onStarEmail={handleStarEmail}
            onArchiveEmail={handleArchiveEmail}
            onDeleteEmail={handleDeleteEmailFromList}
            onMoveOpen={handleMoveOpen}
            onMarkUnread={(id) => markReadMutation.mutate({ id, read: false })}
            onBulkArchive={handleGroupBulkArchive}
            onBulkDelete={handleGroupBulkDelete}
            onBulkMoveOpen={handleBulkMoveOpen}
            onBulkMarkRead={handleGroupBulkMarkRead}
            onBulkMarkUnread={handleGroupBulkMarkUnread}
            isLoading={emailsQuery.isLoading}
            isError={emailsQuery.isError}
            hasNextPage={emailsQuery.hasNextPage}
            isFetchingNextPage={emailsQuery.isFetchingNextPage}
            onFetchNextPage={() => emailsQuery.fetchNextPage()}
          />
        ) : (
          <EmailList
            emails={filteredEmails}
            selectedEmailId={selectedEmailId}
            checkedEmailIds={checkedIds}
            isLoading={emailsQuery.isLoading}
            isError={emailsQuery.isError}
            onSelectEmail={handleSelectEmail}
            onCheckEmail={handleCheckEmail}
            onStarEmail={handleStarEmail}
            onArchiveEmail={handleArchiveEmail}
            onDeleteEmail={handleDeleteEmailFromList}
            onMoveOpen={handleMoveOpen}
            onMarkUnread={(id) => markReadMutation.mutate({ id, read: false })}
            hasNextPage={emailsQuery.hasNextPage}
            isFetchingNextPage={emailsQuery.isFetchingNextPage}
            onFetchNextPage={() => emailsQuery.fetchNextPage()}
          />
        )}
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

      {/* Move to folder dialog */}
      <MoveDialog
        isOpen={isMoveOpen}
        emailSubject={bulkMoveIds ? bulkMoveSubject : (moveDialogEmail?.subject ?? '')}
        labels={labelsQuery.data ?? []}
        onMove={handleMoveConfirm}
        onClose={handleMoveClose}
      />
    </div>
  );
}
