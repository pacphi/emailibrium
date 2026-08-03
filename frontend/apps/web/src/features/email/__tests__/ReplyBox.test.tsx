// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import type { Email } from '@emailibrium/types';
import { ReplyBox } from '../ReplyBox';

afterEach(() => {
  cleanup();
});

const EMAIL: Email = {
  id: 'email-1',
  accountId: 'acct-1',
  provider: 'gmail',
  subject: 'Hello',
  fromAddr: 'sender@example.com',
  toAddrs: 'me@example.com',
  receivedAt: new Date().toISOString(),
  bodyText: 'Hi there',
  isRead: true,
  isStarred: false,
  hasAttachments: false,
  embeddingStatus: 'embedded',
  category: 'primary',
};

const OTHER_EMAIL: Email = {
  ...EMAIL,
  id: 'email-2',
  subject: 'A different thread',
  fromAddr: 'other@example.com',
};

describe('ReplyBox openSignal', () => {
  it('is collapsed by default', () => {
    render(
      <ReplyBox
        originalEmail={EMAIL}
        onSendReply={vi.fn()}
        onSendForward={vi.fn()}
        isSending={false}
      />,
    );

    expect(screen.getByText('Click to reply...')).not.toBeNull();
  });

  it('expands in reply mode when openSignal.mode is "reply"', () => {
    render(
      <ReplyBox
        originalEmail={EMAIL}
        onSendReply={vi.fn()}
        onSendForward={vi.fn()}
        isSending={false}
        openSignal={{ mode: 'reply' }}
      />,
    );

    expect(screen.queryByText('Click to reply...')).toBeNull();
    expect(screen.getByLabelText('Reply message body')).not.toBeNull();
    expect(screen.getByRole('button', { name: 'Reply' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('expands in forward mode when openSignal.mode is "forward"', () => {
    render(
      <ReplyBox
        originalEmail={EMAIL}
        onSendReply={vi.fn()}
        onSendForward={vi.fn()}
        isSending={false}
        openSignal={{ mode: 'forward' }}
      />,
    );

    expect(screen.getByRole('button', { name: 'Forward' }).getAttribute('aria-pressed')).toBe(
      'true',
    );
    expect(screen.getByPlaceholderText('recipient@example.com')).not.toBeNull();
  });

  it('calls onOpenSignalConsumed once after applying the signal', () => {
    const onConsumed = vi.fn();
    render(
      <ReplyBox
        originalEmail={EMAIL}
        onSendReply={vi.fn()}
        onSendForward={vi.fn()}
        isSending={false}
        openSignal={{ mode: 'reply' }}
        onOpenSignalConsumed={onConsumed}
      />,
    );

    expect(onConsumed).toHaveBeenCalledTimes(1);
  });

  it('switches from forward back to reply when a new reply signal arrives while already open', () => {
    const { rerender } = render(
      <ReplyBox
        originalEmail={EMAIL}
        onSendReply={vi.fn()}
        onSendForward={vi.fn()}
        isSending={false}
        openSignal={{ mode: 'forward' }}
      />,
    );
    expect(screen.getByRole('button', { name: 'Forward' }).getAttribute('aria-pressed')).toBe(
      'true',
    );

    rerender(
      <ReplyBox
        originalEmail={EMAIL}
        onSendReply={vi.fn()}
        onSendForward={vi.fn()}
        isSending={false}
        openSignal={{ mode: 'reply' }}
      />,
    );

    expect(screen.getByRole('button', { name: 'Reply' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('regression: resets a stale draft instead of carrying it over when the underlying email changes without unmounting', () => {
    const onSendForward = vi.fn();
    const { rerender } = render(
      <ReplyBox
        originalEmail={EMAIL}
        onSendReply={vi.fn()}
        onSendForward={onSendForward}
        isSending={false}
        openSignal={{ mode: 'forward' }}
      />,
    );
    const forwardInput = screen.getByPlaceholderText('recipient@example.com') as HTMLInputElement;
    forwardInput.value = 'draft-recipient@example.com';
    forwardInput.dispatchEvent(new Event('input', { bubbles: true }));

    // Switch to a different email in the same ReplyBox instance (no unmount) -- e.g. the
    // user clicked a different row in the list without sending/discarding first.
    rerender(
      <ReplyBox
        originalEmail={OTHER_EMAIL}
        onSendReply={vi.fn()}
        onSendForward={onSendForward}
        isSending={false}
        openSignal={null}
      />,
    );

    expect(screen.getByText('Click to reply...')).not.toBeNull();
    expect(screen.queryByPlaceholderText('recipient@example.com')).toBeNull();
  });

  it('does not expand or throw when openSignal is null/absent', () => {
    render(
      <ReplyBox
        originalEmail={EMAIL}
        onSendReply={vi.fn()}
        onSendForward={vi.fn()}
        isSending={false}
        openSignal={null}
      />,
    );

    expect(screen.getByText('Click to reply...')).not.toBeNull();
  });
});
