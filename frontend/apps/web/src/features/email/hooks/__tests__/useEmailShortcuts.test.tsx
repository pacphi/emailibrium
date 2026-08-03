// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, act } from '@testing-library/react';
import { createElement } from 'react';
import { useEmailShortcuts } from '../useEmailShortcuts';

afterEach(() => {
  cleanup();
});

function press(key: string) {
  act(() => {
    document.body.dispatchEvent(
      new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }),
    );
  });
}

function mount(
  selectedEmailId: string | null,
  onCompose: () => void,
  onOpenReply: (s: unknown) => void,
) {
  function Harness() {
    useEmailShortcuts({ selectedEmailId, onCompose, onOpenReply });
    return null;
  }
  return render(createElement(Harness));
}

describe('useEmailShortcuts', () => {
  it('"c" opens compose regardless of whether an email is selected', () => {
    const onCompose = vi.fn();
    mount(null, onCompose, vi.fn());

    press('c');

    expect(onCompose).toHaveBeenCalledTimes(1);
  });

  it('"c" opens compose even with an email selected', () => {
    const onCompose = vi.fn();
    mount('email-1', onCompose, vi.fn());

    press('c');

    expect(onCompose).toHaveBeenCalledTimes(1);
  });

  it('"r" opens reply mode when an email is selected', () => {
    const onOpenReply = vi.fn();
    mount('email-1', vi.fn(), onOpenReply);

    press('r');

    expect(onOpenReply).toHaveBeenCalledWith({ mode: 'reply' });
  });

  it('"r" is a no-op with nothing selected', () => {
    const onOpenReply = vi.fn();
    mount(null, vi.fn(), onOpenReply);

    press('r');

    expect(onOpenReply).not.toHaveBeenCalled();
  });

  it('"f" opens forward mode when an email is selected', () => {
    const onOpenReply = vi.fn();
    mount('email-1', vi.fn(), onOpenReply);

    press('f');

    expect(onOpenReply).toHaveBeenCalledWith({ mode: 'forward' });
  });

  it('"f" is a no-op with nothing selected', () => {
    const onOpenReply = vi.fn();
    mount(null, vi.fn(), onOpenReply);

    press('f');

    expect(onOpenReply).not.toHaveBeenCalled();
  });

  it('does not trigger "c" while typing in an editable field (reuses useKeyboard\'s editable-field guard)', () => {
    const onCompose = vi.fn();
    mount(null, onCompose, vi.fn());
    const input = document.createElement('input');
    document.body.appendChild(input);

    act(() => {
      input.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'c', bubbles: true, cancelable: true }),
      );
    });

    expect(onCompose).not.toHaveBeenCalled();
    document.body.removeChild(input);
  });

  it('re-registers when selectedEmailId changes from null to a real id', () => {
    const onOpenReply = vi.fn();
    function Harness({ selectedEmailId }: { selectedEmailId: string | null }) {
      useEmailShortcuts({ selectedEmailId, onCompose: vi.fn(), onOpenReply });
      return null;
    }
    const { rerender } = render(createElement(Harness, { selectedEmailId: null }));
    press('r');
    expect(onOpenReply).not.toHaveBeenCalled();

    rerender(createElement(Harness, { selectedEmailId: 'email-2' }));
    press('r');

    expect(onOpenReply).toHaveBeenCalledWith({ mode: 'reply' });
  });
});
