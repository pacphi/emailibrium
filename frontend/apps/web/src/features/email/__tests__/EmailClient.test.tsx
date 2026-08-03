// @vitest-environment jsdom
//
// Wiring tests: render the REAL EmailClient (and the real useEmailShortcuts /
// useKeyboard underneath it) and assert that a keypress reaches the REAL handler and
// the right mutation -- not that some injected mock fires. Child components are stubbed
// to tiny probes and the mutation hooks are spied at the module boundary, so what's
// under test is exactly the layer the hook-level tests can't see: EmailClient's own
// handler wiring, selection bookkeeping, per-view rules, and modal suppression.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, cleanup, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { Email } from '@emailibrium/types';
import { press } from '@test-utils/press';
import { useToastStore } from '@/shared/stores/toastStore';

const mocks = vi.hoisted(() => {
  function email(id: string): Email {
    return {
      id,
      accountId: 'acct-1',
      provider: 'gmail',
      subject: `Subject ${id}`,
      fromAddr: 'sender@example.com',
      toAddrs: 'me@example.com',
      receivedAt: new Date('2026-01-01').toISOString(),
      isRead: true,
      isStarred: false,
      hasAttachments: false,
      embeddingStatus: 'embedded',
      category: 'primary',
    };
  }
  return {
    emails: [email('email-1'), email('email-2'), email('email-3')],
    archiveMutate: vi.fn(),
    deleteMutate: vi.fn(),
    permanentDeleteMutate: vi.fn(),
  };
});

vi.mock('../hooks/useEmails', () => ({
  useEmailsQuery: () => ({
    data: { pages: [{ emails: mocks.emails }] },
    isLoading: false,
    isError: false,
    hasNextPage: false,
    isFetchingNextPage: false,
    fetchNextPage: vi.fn(),
    refetch: vi.fn(),
  }),
  useEmailQuery: () => ({ data: null }),
  useThreadQuery: () => ({ data: undefined, isLoading: false, isError: false }),
  useArchiveEmail: () => ({ mutate: mocks.archiveMutate }),
  useStarEmail: () => ({ mutate: vi.fn() }),
  useDeleteEmail: () => ({ mutate: mocks.deleteMutate }),
  useReplyToEmail: () => ({ mutate: vi.fn(), isPending: false }),
  useForwardEmail: () => ({ mutate: vi.fn(), isPending: false }),
  useLabelsQuery: () => ({ data: [] }),
  useMoveEmail: () => ({ mutate: vi.fn() }),
  useMarkRead: () => ({ mutate: vi.fn() }),
  useMarkAsSpam: () => ({ mutate: vi.fn() }),
  useUnmarkSpam: () => ({ mutate: vi.fn() }),
  useRestoreEmail: () => ({ mutate: vi.fn() }),
  useEmptyTrash: () => ({ mutate: vi.fn(), isPending: false }),
  usePermanentDelete: () => ({ mutate: mocks.permanentDeleteMutate }),
}));

vi.mock('@emailibrium/api', () => ({
  submitFeedback: vi.fn(),
  getAllLabels: vi.fn().mockResolvedValue([]),
  getEnrichedCategories: vi.fn().mockResolvedValue([]),
  getEmailCounts: vi.fn().mockResolvedValue({ total: 3, unread: 0, archivedCount: 0 }),
}));

/* eslint-disable @typescript-eslint/no-explicit-any -- child stubs echo untyped props */
vi.mock('../EmailSidebar', () => ({ EmailSidebar: () => null }));
vi.mock('../GroupedEmailList', () => ({ GroupedEmailList: () => null }));
vi.mock('../MoveDialog', () => ({ MoveDialog: () => null }));
vi.mock('../EmailList', () => ({
  EmailList: (props: any) => (
    <div>
      <div data-testid="checked-count">{props.checkedEmailIds.size}</div>
      {props.emails.map((e: any) => (
        <div key={e.id}>
          <button type="button" onClick={() => props.onSelectEmail(e.id)}>
            select-{e.id}
          </button>
          <button type="button" onClick={() => props.onCheckEmail(e.id, true)}>
            check-{e.id}
          </button>
        </div>
      ))}
    </div>
  ),
}));
vi.mock('../ThreadView', () => ({
  ThreadView: (props: any) => (
    <div data-testid="thread-view" data-reply-mode={props.replyOpenSignal?.mode ?? 'none'} />
  ),
}));
vi.mock('../ComposeEmail', () => ({
  ComposeEmail: (props: any) => (props.isOpen ? <div data-testid="compose-open" /> : null),
}));
/* eslint-enable @typescript-eslint/no-explicit-any */

import { EmailClient } from '../EmailClient';

function renderClient() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <EmailClient />
    </QueryClientProvider>,
  );
}

function selectEmail(id: string) {
  fireEvent.click(screen.getByText(`select-${id}`));
}

function checkEmail(id: string) {
  fireEvent.click(screen.getByText(`check-${id}`));
}

describe('EmailClient keyboard wiring', () => {
  beforeEach(() => {
    mocks.archiveMutate.mockClear();
    mocks.deleteMutate.mockClear();
    mocks.permanentDeleteMutate.mockClear();
    useToastStore.getState().clearAll();
  });

  afterEach(() => {
    cleanup();
  });

  it('"e" with a selected email runs the real archive mutation with that id', () => {
    renderClient();
    selectEmail('email-1');

    press('e');

    expect(mocks.archiveMutate).toHaveBeenCalledTimes(1);
    expect(mocks.archiveMutate).toHaveBeenCalledWith('email-1');
  });

  it('archiving via "e" clears the selection: a follow-up "r" no longer opens a reply for the gone email', () => {
    renderClient();
    selectEmail('email-1');

    press('e');
    press('r');

    expect(screen.getByTestId('thread-view').getAttribute('data-reply-mode')).toBe('none');
  });

  it('"r" with a selected email opens the reply editor through the real signal wiring', () => {
    renderClient();
    selectEmail('email-1');

    press('r');

    expect(screen.getByTestId('thread-view').getAttribute('data-reply-mode')).toBe('reply');
  });

  it('"#" with a single selected email runs the real delete mutation directly (no confirmation)', () => {
    renderClient();
    selectEmail('email-2');

    press('#', { shiftKey: true });

    expect(mocks.deleteMutate).toHaveBeenCalledTimes(1);
    expect(mocks.deleteMutate).toHaveBeenCalledWith('email-2');
  });

  it('cmd+shift+a checks every visible email through the real select-all wiring, with count feedback', () => {
    renderClient();

    press('a', { metaKey: true, shiftKey: true });

    expect(screen.getByTestId('checked-count').textContent).toBe('3');
    const toasts = useToastStore.getState().toasts;
    expect(toasts.some((t) => t.message === 'Selected 3 emails')).toBe(true);
  });

  it('a bulk "#" (more than one email checked) asks for confirmation BEFORE any mutation runs', () => {
    renderClient();
    checkEmail('email-1');
    checkEmail('email-2');

    press('#', { shiftKey: true });

    expect(screen.getByRole('dialog', { name: 'Confirm action' })).not.toBeNull();
    expect(mocks.deleteMutate).not.toHaveBeenCalled();
  });

  it('confirming the bulk dialog runs the mutation for every checked email and clears the check-set', () => {
    renderClient();
    checkEmail('email-1');
    checkEmail('email-2');
    press('#', { shiftKey: true });

    fireEvent.click(screen.getByRole('button', { name: 'Move to trash' }));

    expect(mocks.deleteMutate).toHaveBeenCalledTimes(2);
    expect(mocks.deleteMutate).toHaveBeenCalledWith('email-1');
    expect(mocks.deleteMutate).toHaveBeenCalledWith('email-2');
    expect(screen.getByTestId('checked-count').textContent).toBe('0');
  });

  it('cancelling the bulk dialog runs nothing and keeps the selection', () => {
    renderClient();
    checkEmail('email-1');
    checkEmail('email-2');
    press('#', { shiftKey: true });

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(mocks.deleteMutate).not.toHaveBeenCalled();
    expect(screen.getByTestId('checked-count').textContent).toBe('2');
  });

  it('a bulk "e" (archive) also confirms first, then archives every checked email', () => {
    renderClient();
    checkEmail('email-1');
    checkEmail('email-3');

    press('e');
    expect(mocks.archiveMutate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Archive' }));

    expect(mocks.archiveMutate).toHaveBeenCalledTimes(2);
    expect(mocks.archiveMutate).toHaveBeenCalledWith('email-1');
    expect(mocks.archiveMutate).toHaveBeenCalledWith('email-3');
  });

  it('regression (compose focus leak): while the Compose modal is open, "e" cannot archive the email behind it', () => {
    renderClient();
    selectEmail('email-1');

    press('c');
    expect(screen.getByTestId('compose-open')).not.toBeNull();
    press('e');
    press('#', { shiftKey: true });

    expect(mocks.archiveMutate).not.toHaveBeenCalled();
    expect(mocks.deleteMutate).not.toHaveBeenCalled();
  });

  it('in the Trash view, "e" is not registered (the action bar offers no Archive there)', () => {
    renderClient();
    fireEvent.click(screen.getByText('Trash'));
    selectEmail('email-1');

    press('e');

    expect(mocks.archiveMutate).not.toHaveBeenCalled();
  });

  it('in the Trash view, "#" routes to the confirmation-gated PERMANENT delete, matching the visible action bar', () => {
    renderClient();
    fireEvent.click(screen.getByText('Trash'));
    selectEmail('email-1');

    press('#', { shiftKey: true });
    expect(mocks.permanentDeleteMutate).not.toHaveBeenCalled();
    expect(mocks.deleteMutate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Delete permanently' }));

    expect(mocks.permanentDeleteMutate).toHaveBeenCalledTimes(1);
    expect(mocks.permanentDeleteMutate).toHaveBeenCalledWith('email-1');
    expect(mocks.deleteMutate).not.toHaveBeenCalled();
  });
});
