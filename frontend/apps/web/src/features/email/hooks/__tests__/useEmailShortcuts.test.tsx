// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, act } from '@testing-library/react';
import { createElement } from 'react';
import { useEmailShortcuts } from '../useEmailShortcuts';
import { press } from '@/shared/test-utils/press';

afterEach(() => {
  cleanup();
});

interface MountOverrides {
  selectedEmailId?: string | null;
  onCompose?: () => void;
  onOpenReply?: (s: unknown) => void;
  onArchive?: () => void;
  onDelete?: () => void;
  onSelectAll?: () => void;
}

function mount(overrides: MountOverrides = {}) {
  const args = {
    selectedEmailId: overrides.selectedEmailId ?? null,
    onCompose: overrides.onCompose ?? vi.fn(),
    onOpenReply: overrides.onOpenReply ?? vi.fn(),
    onArchive: overrides.onArchive ?? vi.fn(),
    onDelete: overrides.onDelete ?? vi.fn(),
    onSelectAll: overrides.onSelectAll ?? vi.fn(),
  };
  function Harness() {
    useEmailShortcuts(args);
    return null;
  }
  return { ...render(createElement(Harness)), args };
}

describe('useEmailShortcuts', () => {
  it('"c" opens compose regardless of whether an email is selected', () => {
    const onCompose = vi.fn();
    mount({ selectedEmailId: null, onCompose });

    press('c');

    expect(onCompose).toHaveBeenCalledTimes(1);
  });

  it('"c" opens compose even with an email selected', () => {
    const onCompose = vi.fn();
    mount({ selectedEmailId: 'email-1', onCompose });

    press('c');

    expect(onCompose).toHaveBeenCalledTimes(1);
  });

  it('"r" opens reply mode when an email is selected', () => {
    const onOpenReply = vi.fn();
    mount({ selectedEmailId: 'email-1', onOpenReply });

    press('r');

    expect(onOpenReply).toHaveBeenCalledWith({ mode: 'reply' });
  });

  it('"r" is a no-op with nothing selected', () => {
    const onOpenReply = vi.fn();
    mount({ selectedEmailId: null, onOpenReply });

    press('r');

    expect(onOpenReply).not.toHaveBeenCalled();
  });

  it('"f" opens forward mode when an email is selected', () => {
    const onOpenReply = vi.fn();
    mount({ selectedEmailId: 'email-1', onOpenReply });

    press('f');

    expect(onOpenReply).toHaveBeenCalledWith({ mode: 'forward' });
  });

  it('"f" is a no-op with nothing selected', () => {
    const onOpenReply = vi.fn();
    mount({ selectedEmailId: null, onOpenReply });

    press('f');

    expect(onOpenReply).not.toHaveBeenCalled();
  });

  it('"e" fires the same onArchive the thread action bar\'s Archive button calls', () => {
    const onArchive = vi.fn();
    mount({ onArchive });

    press('e');

    expect(onArchive).toHaveBeenCalledTimes(1);
  });

  it('"#" (shift+3) fires the same onDelete the thread action bar\'s Delete button calls', () => {
    const onDelete = vi.fn();
    mount({ onDelete });

    press('#', { shiftKey: true });

    expect(onDelete).toHaveBeenCalledTimes(1);
  });

  it('bare "#" without the shift flag does not fire (useKeyboard requires the exact modifier match)', () => {
    const onDelete = vi.fn();
    mount({ onDelete });

    press('#');

    expect(onDelete).not.toHaveBeenCalled();
  });

  it('cmd+shift+a fires onSelectAll', () => {
    const onSelectAll = vi.fn();
    mount({ onSelectAll });

    press('a', { metaKey: true, shiftKey: true });

    expect(onSelectAll).toHaveBeenCalledTimes(1);
  });

  it('ctrl+shift+a also fires onSelectAll (Windows/Linux parity, matching cmd+k/ctrl+k)', () => {
    const onSelectAll = vi.fn();
    mount({ onSelectAll });

    press('a', { ctrlKey: true, shiftKey: true });

    expect(onSelectAll).toHaveBeenCalledTimes(1);
  });

  it('does not trigger "e" while typing in an editable field (reuses useKeyboard\'s editable-field guard)', () => {
    const onArchive = vi.fn();
    mount({ onArchive });
    const input = document.createElement('input');
    document.body.appendChild(input);

    act(() => {
      input.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'e', bubbles: true, cancelable: true }),
      );
    });

    expect(onArchive).not.toHaveBeenCalled();
    document.body.removeChild(input);
  });

  it('cmd+shift+a still fires while typing in an editable field (modifier shortcuts bypass the guard)', () => {
    const onSelectAll = vi.fn();
    mount({ onSelectAll });
    const input = document.createElement('input');
    document.body.appendChild(input);

    act(() => {
      input.dispatchEvent(
        new KeyboardEvent('keydown', {
          key: 'a',
          metaKey: true,
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    expect(onSelectAll).toHaveBeenCalledTimes(1);
    document.body.removeChild(input);
  });

  it('does not trigger "c" while typing in an editable field (reuses useKeyboard\'s editable-field guard)', () => {
    const onCompose = vi.fn();
    mount({ onCompose });
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
      useEmailShortcuts({
        selectedEmailId,
        onCompose: vi.fn(),
        onOpenReply,
        onArchive: vi.fn(),
        onDelete: vi.fn(),
        onSelectAll: vi.fn(),
      });
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
