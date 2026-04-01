import { useState, useCallback, useMemo, useEffect, useRef } from 'react';
import { Plus, Trash2, ArrowLeft, Search } from 'lucide-react';
import { submitFeedback } from '@emailibrium/api';
import type { EmailViewContext } from './EmailActions';
import { EmailSidebar } from './EmailSidebar';
import type { SidebarGroup } from './EmailSidebar';
import { EmailList } from './EmailList';
import { GroupedEmailList } from './GroupedEmailList';
import { groupByDomain } from './utils/groupBySender';
import { ThreadView } from './ThreadView';
import { ComposeEmail } from './ComposeEmail';
import { MoveDialog } from './MoveDialog';
import { useQuery } from '@tanstack/react-query';
import { getAllLabels, getEnrichedCategories, getEmailCounts } from '@emailibrium/api';
import {
  useEmailsQuery,
  useEmailQuery,
  useThreadQuery,
  useArchiveEmail,
  useStarEmail,
  useDeleteEmail,
  useReplyToEmail,
  useForwardEmail,
  useBulkArchive,
  useBulkDelete,
  useLabelsQuery,
  useMoveEmail,
  useMarkRead,
  useMarkAsSpam,
  useUnmarkSpam,
  useRestoreEmail,
  useEmptyTrash,
  usePermanentDelete,
} from './hooks/useEmails';
import { useToast } from '@/shared/hooks/useToast';

function groupToQueryParam(groupId: string): { category?: string; label?: string } {
  if (groupId === 'inbox') return {};
  if (groupId.startsWith('cat-')) return { category: groupId.replace('cat-', '') };
  if (groupId.startsWith('topic-')) return { category: groupId.replace('topic-', '') };
  if (groupId.startsWith('sub-')) return { category: groupId.replace('sub-', '') };
  if (groupId.startsWith('label-')) return { label: groupId.replace('label-', '') };
  return {};
}

export function EmailClient() {
  const [activeGroup, setActiveGroup] = useState('inbox');
  const [selectedEmailId, setSelectedEmailId] = useState<string | null>(null);
  const [checkedIds, setCheckedIds] = useState<Set<string>>(new Set());
  const [isComposeOpen, setIsComposeOpen] = useState(false);
  const [mobilePanel, setMobilePanel] = useState<'sidebar' | 'list' | 'thread'>('list');
  const [filter, setFilter] = useState<'all' | 'read' | 'unread' | 'starred' | 'spam' | 'trash'>(
    'all',
  );
  const [isGrouped, setIsGrouped] = useState(false);
  const [expandedDomains, setExpandedDomains] = useState<Set<string>>(new Set());
  const [expandedSenders, setExpandedSenders] = useState<Set<string>>(new Set());
  const [searchText, setSearchText] = useState('');
  const [searchField, setSearchField] = useState<'from' | 'to' | 'cc' | 'subject' | 'body'>('from');
  const [fromSearch, setFromSearch] = useState(false);
  const [scrollToEmailId, setScrollToEmailId] = useState<string | null>(null);

  // Read URL params on mount (search result deep-link or topic/category deep-link).
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const urlEmailId = params.get('id');
    const urlGroup = params.get('group');
    if (urlEmailId) {
      setSelectedEmailId(urlEmailId);
      setFromSearch(true);
      setScrollToEmailId(urlEmailId);
      setMobilePanel('thread');
    }
    if (urlGroup) {
      setActiveGroup(urlGroup);
    }
    // Clean the URL so refreshing doesn't re-trigger.
    if (urlEmailId || urlGroup) {
      window.history.replaceState({}, '', '/email');
    }
  }, []);

  const queryParams = useMemo(() => {
    const base = groupToQueryParam(activeGroup);
    if (filter === 'read') return { ...base, isRead: true };
    if (filter === 'unread') return { ...base, isRead: false };
    if (filter === 'starred') return { ...base, isStarred: true };
    if (filter === 'spam') return { ...base, isSpam: true };
    if (filter === 'trash') return { ...base, isTrash: true };
    return base;
  }, [activeGroup, filter]);
  const emailsQuery = useEmailsQuery(queryParams);
  const emails = useMemo(
    () => emailsQuery.data?.pages.flatMap((p) => p.emails) ?? [],
    [emailsQuery.data?.pages],
  );

  // Progressive page loading: when we need to scroll to an email that's not yet
  // loaded (e.g. from a search result deep-link), keep fetching pages until found.
  const fetchingForScrollRef = useRef(false);
  useEffect(() => {
    if (!scrollToEmailId) return;
    // Wait for initial load to complete before doing anything.
    if (emailsQuery.isLoading) return;
    const found = emails.some((e) => e.id === scrollToEmailId);
    if (found) {
      fetchingForScrollRef.current = false;
      return;
    }
    // Email not in loaded pages — fetch more if available.
    if (emailsQuery.hasNextPage && !emailsQuery.isFetchingNextPage) {
      fetchingForScrollRef.current = true;
      emailsQuery.fetchNextPage();
    } else if (!emailsQuery.hasNextPage && !emailsQuery.isFetchingNextPage) {
      // Exhausted all pages without finding the email — clear scroll target.
      fetchingForScrollRef.current = false;
      setScrollToEmailId(null);
    }
  }, [scrollToEmailId, emails, emailsQuery]);

  const filteredEmails = useMemo(() => {
    if (!searchText.trim()) return emails;
    const needle = searchText.toLowerCase();
    return emails.filter((email) => {
      switch (searchField) {
        case 'from':
          return (
            email.fromAddr.toLowerCase().includes(needle) ||
            (email.fromName?.toLowerCase().includes(needle) ?? false)
          );
        case 'to':
          return email.toAddrs.toLowerCase().includes(needle);
        case 'cc':
          return email.ccAddrs?.toLowerCase().includes(needle) ?? false;
        case 'subject':
          return email.subject.toLowerCase().includes(needle);
        case 'body':
          return email.bodyText?.toLowerCase().includes(needle) ?? false;
        default:
          return true;
      }
    });
  }, [emails, searchText, searchField]);

  const selectedEmailFromList = emails.find((e) => e.id === selectedEmailId) ?? null;
  // Fallback: fetch individual email when it's not in the current list (e.g. from search).
  const directEmailQuery = useEmailQuery(selectedEmailFromList ? null : selectedEmailId);
  const selectedEmail = selectedEmailFromList ?? directEmailQuery.data ?? null;
  const threadId = selectedEmail?.threadId ?? selectedEmailId;
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
  const markAsSpamMutation = useMarkAsSpam();
  const unmarkSpamMutation = useUnmarkSpam();
  const restoreEmailMutation = useRestoreEmail();
  const emptyTrashMutation = useEmptyTrash();
  const permanentDeleteMutation = usePermanentDelete();
  const { toast } = useToast();

  // Determine account ID from the first email for label fetching.
  const currentAccountId = emails.length > 0 ? emails[0]?.accountId : undefined;
  const labelsQuery = useLabelsQuery(currentAccountId);

  // Fetch enriched categories with group assignments and counts (Gap 5 + Gap 6).
  const enrichedQuery = useQuery({
    queryKey: ['categories-enriched'],
    queryFn: () => getEnrichedCategories(),
    staleTime: 30_000,
  });

  // Fetch aggregated labels across all accounts (Gap 1 + Gap 4).
  const allLabelsQuery = useQuery({
    queryKey: ['labels-all'],
    queryFn: () => getAllLabels(),
    staleTime: 30_000,
  });

  // Fetch accurate counts (Gap 6).
  const countsQuery = useQuery({
    queryKey: ['email-counts'],
    queryFn: () => getEmailCounts(),
    staleTime: 10_000,
    refetchInterval: 30_000,
  });

  // Build sidebar groups dynamically from enriched categories + labels.
  const groupsWithCounts = useMemo(() => {
    const enriched = [...(enrichedQuery.data ?? [])].sort((a, b) => a.name.localeCompare(b.name));
    const labels = (allLabelsQuery.data ?? [])
      .filter((l) => !l.isSystem && l.emailCount > 0)
      .sort((a, b) => a.name.localeCompare(b.name));
    const counts = countsQuery.data;
    const inboxUnread = counts?.unread ?? 0;

    const groups: SidebarGroup[] = [
      { id: 'inbox', label: 'Inbox', icon: 'inbox', unreadCount: inboxUnread },
    ];

    // Categories from enriched endpoint (dynamic grouping from backend).
    for (const cat of enriched) {
      const icon = cat.group === 'subscription' ? ('subscription' as const) : ('category' as const);
      const prefix = cat.group === 'subscription' ? 'sub-' : 'cat-';
      groups.push({
        id: `${prefix}${cat.name}`,
        label: cat.name,
        icon,
        unreadCount: cat.emailCount,
      });
    }

    // Provider labels (Gap 1 — works without AI, separate section).
    for (const label of labels) {
      groups.push({
        id: `label-${label.name}`,
        label: label.name,
        icon: 'label',
        unreadCount: label.emailCount,
      });
    }

    return groups;
  }, [enrichedQuery.data, allLabelsQuery.data, countsQuery.data]);

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

  const handleGroupSelect = useCallback(
    (groupId: string) => {
      // Toggle off if clicking the already-active filter (but not inbox — it's the default).
      if (groupId === activeGroup && groupId !== 'inbox') {
        setActiveGroup('inbox');
      } else {
        setActiveGroup(groupId);
      }
      setSelectedEmailId(null);
      setCheckedIds(new Set());
      setMobilePanel('list');
    },
    [activeGroup],
  );

  const handleSelectEmail = useCallback(
    (emailId: string) => {
      setSelectedEmailId(emailId);
      setScrollToEmailId(emailId);
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

  // Derive the current view context for the actions bar.
  const viewContext: EmailViewContext = useMemo(() => {
    if (filter === 'spam') return 'spam';
    if (filter === 'trash') return 'trash';
    return 'inbox';
  }, [filter]);

  const handleSpamEmail = useCallback(
    (emailId: string) => {
      markAsSpamMutation.mutate(emailId, {
        onSuccess: () => toast('Email marked as spam', 'success'),
      });
      if (selectedEmailId === emailId) setSelectedEmailId(null);
    },
    [markAsSpamMutation, selectedEmailId, toast],
  );

  const handleRestoreEmail = useCallback(
    (emailId: string) => {
      if (filter === 'spam') {
        unmarkSpamMutation.mutate(emailId, {
          onSuccess: () => toast('Email restored from spam', 'success'),
        });
      } else {
        restoreEmailMutation.mutate(emailId, {
          onSuccess: () => toast('Email restored from trash', 'success'),
        });
      }
      if (selectedEmailId === emailId) setSelectedEmailId(null);
    },
    [filter, unmarkSpamMutation, restoreEmailMutation, selectedEmailId, toast],
  );

  const handlePermanentDelete = useCallback(
    (emailId: string) => {
      permanentDeleteMutation.mutate(emailId, {
        onSuccess: () => toast('Email permanently deleted', 'success'),
      });
      if (selectedEmailId === emailId) setSelectedEmailId(null);
    },
    [permanentDeleteMutation, selectedEmailId, toast],
  );

  const [showEmptyTrashConfirm, setShowEmptyTrashConfirm] = useState(false);

  const handleEmptyTrash = useCallback(() => {
    emptyTrashMutation.mutate(undefined, {
      onSuccess: () => {
        toast('Trash emptied', 'success');
        setShowEmptyTrashConfirm(false);
      },
    });
  }, [emptyTrashMutation, toast]);

  // Move dialog state — supports single email or bulk.
  const [moveDialogEmailId, setMoveDialogEmailId] = useState<string | null>(null);
  const [bulkMoveIds, setBulkMoveIds] = useState<string[] | null>(null);
  const [bulkMoveSubject, setBulkMoveSubject] = useState('');
  const moveDialogEmail = moveDialogEmailId ? emails.find((e) => e.id === moveDialogEmailId) : null;
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

  const handleThreadSpam = useCallback(() => {
    if (selectedEmailId) {
      handleSpamEmail(selectedEmailId);
    }
  }, [selectedEmailId, handleSpamEmail]);

  const handleThreadRestore = useCallback(() => {
    if (selectedEmailId) {
      handleRestoreEmail(selectedEmailId);
    }
  }, [selectedEmailId, handleRestoreEmail]);

  const handleThreadPermanentDelete = useCallback(() => {
    if (selectedEmailId) {
      handlePermanentDelete(selectedEmailId);
    }
  }, [selectedEmailId, handlePermanentDelete]);

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
              {groupsWithCounts.find((g: SidebarGroup) => g.id === activeGroup)?.label ?? 'Inbox'}
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
          <div className="flex flex-wrap items-center gap-1 px-3 pb-2">
            {(['all', 'unread', 'read', 'starred'] as const).map((f) => (
              <button
                key={f}
                type="button"
                onClick={() => setFilter((prev) => (prev === f ? 'all' : f))}
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
            {/* Spam pill */}
            <button
              type="button"
              onClick={() => setFilter((prev) => (prev === 'spam' ? 'all' : 'spam'))}
              className={`rounded-full px-3 py-1 text-xs font-medium transition-colors ${
                filter === 'spam'
                  ? 'bg-amber-500 text-white'
                  : 'bg-gray-100 text-gray-600 hover:bg-gray-200 dark:bg-gray-700 dark:text-gray-300 dark:hover:bg-gray-600'
              }`}
            >
              Spam{countsQuery.data?.spam_count != null ? ` (${countsQuery.data.spam_count})` : ''}
            </button>
            {/* Trash pill */}
            <button
              type="button"
              onClick={() => setFilter((prev) => (prev === 'trash' ? 'all' : 'trash'))}
              className={`rounded-full px-3 py-1 text-xs font-medium transition-colors ${
                filter === 'trash'
                  ? 'bg-red-600 text-white'
                  : 'bg-gray-100 text-gray-600 hover:bg-gray-200 dark:bg-gray-700 dark:text-gray-300 dark:hover:bg-gray-600'
              }`}
            >
              Trash
              {countsQuery.data?.trash_count != null ? ` (${countsQuery.data.trash_count})` : ''}
            </button>
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
          {/* Empty Trash button -- visible only in Trash view */}
          {filter === 'trash' && (
            <div className="flex items-center gap-2 px-3 pb-2">
              {!showEmptyTrashConfirm ? (
                <button
                  type="button"
                  onClick={() => setShowEmptyTrashConfirm(true)}
                  className="flex items-center gap-1 rounded-md bg-red-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-red-700"
                >
                  <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
                  Empty Trash
                </button>
              ) : (
                <div className="flex items-center gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 dark:border-red-800 dark:bg-red-900/20">
                  <span className="text-xs text-red-700 dark:text-red-300">
                    Permanently delete all
                    {countsQuery.data?.trash_count != null
                      ? ` ${countsQuery.data.trash_count}`
                      : ''}{' '}
                    emails in trash?
                  </span>
                  <button
                    type="button"
                    onClick={handleEmptyTrash}
                    disabled={emptyTrashMutation.isPending}
                    className="rounded-md bg-red-600 px-2.5 py-1 text-xs font-medium text-white hover:bg-red-700 disabled:opacity-50"
                  >
                    {emptyTrashMutation.isPending ? 'Deleting...' : 'Yes, delete all'}
                  </button>
                  <button
                    type="button"
                    onClick={() => setShowEmptyTrashConfirm(false)}
                    className="rounded-md bg-gray-100 px-2.5 py-1 text-xs font-medium text-gray-700 hover:bg-gray-200 dark:bg-gray-700 dark:text-gray-300"
                  >
                    Cancel
                  </button>
                </div>
              )}
            </div>
          )}
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
            scrollToEmailId={scrollToEmailId}
            onScrollToComplete={() => setScrollToEmailId(null)}
          />
        )}
      </div>

      {/* Right panel: thread view */}
      <div
        className={`${
          mobilePanel === 'thread' ? 'flex' : 'hidden'
        } min-w-0 flex-1 flex-col lg:flex`}
      >
        {fromSearch && selectedEmailId && (
          <div className="flex items-center gap-2 border-b border-indigo-200 bg-indigo-50 px-4 py-2 dark:border-indigo-800 dark:bg-indigo-900/30">
            <Search className="h-4 w-4 text-indigo-600 dark:text-indigo-400" aria-hidden="true" />
            <span className="text-xs font-medium text-indigo-700 dark:text-indigo-300">
              Viewing search result
            </span>
            <a
              href="/command-center?view=search"
              className="ml-auto flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium text-indigo-600 hover:bg-indigo-100 dark:text-indigo-400 dark:hover:bg-indigo-800/50"
            >
              <ArrowLeft className="h-3 w-3" aria-hidden="true" />
              Back to search
            </a>
            <button
              type="button"
              onClick={() => {
                setFromSearch(false);
                setScrollToEmailId(selectedEmailId);
              }}
              className="rounded-md px-2 py-1 text-xs font-medium text-gray-500 hover:bg-gray-100 dark:text-gray-400 dark:hover:bg-gray-700"
            >
              Show in inbox
            </button>
          </div>
        )}
        <ThreadView
          thread={threadQuery.data}
          isLoading={threadQuery.isLoading}
          isError={threadQuery.isError}
          viewContext={viewContext}
          onBack={handleBackToList}
          onArchive={handleThreadArchive}
          onStar={handleThreadStar}
          onDelete={handleThreadDelete}
          onReclassify={handleReclassify}
          onMove={handleMove}
          onSendReply={handleSendReply}
          onSendForward={handleSendForward}
          isSendingReply={replyMutation.isPending || forwardMutation.isPending}
          onSpam={handleThreadSpam}
          onRestore={handleThreadRestore}
          onPermanentDelete={handleThreadPermanentDelete}
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
