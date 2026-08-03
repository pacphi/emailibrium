// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, cleanup, act } from '@testing-library/react';
import { createElement } from 'react';
import { useKeyboard } from '@/shared/hooks';
import { ShortcutHelpPanel, formatShortcutKey, buildShortcutRows } from '../ShortcutHelpPanel';
import { useShortcutHelpStore } from '../hooks/useShortcutHelp';

afterEach(() => {
  cleanup();
});

describe('formatShortcutKey', () => {
  it('formats a single key with no modifiers', () => {
    expect(formatShortcutKey('e')).toBe('E');
  });

  it('formats a cmd-modified key with the Mac symbol', () => {
    expect(formatShortcutKey('cmd+k')).toBe('⌘+K');
  });

  it('formats a ctrl-modified key as "Ctrl"', () => {
    expect(formatShortcutKey('ctrl+k')).toBe('Ctrl+K');
  });

  it('formats multiple modifiers in order', () => {
    expect(formatShortcutKey('cmd+shift+a')).toBe('⌘+⇧+A');
  });

  it('leaves a symbol key (not a letter) as-is', () => {
    expect(formatShortcutKey('#')).toBe('#');
  });

  it('capitalizes a multi-character key name ("escape" renders as "Escape")', () => {
    expect(formatShortcutKey('escape')).toBe('Escape');
  });
});

describe('buildShortcutRows', () => {
  it('groups keys that share the same known label into one row', () => {
    const rows = buildShortcutRows(['cmd+k', 'ctrl+k']);

    expect(rows).toHaveLength(1);
    expect(rows[0]?.label).toBe('Open command palette');
    expect(rows[0]?.keys.sort()).toEqual(['cmd+k', 'ctrl+k']);
  });

  it('sorts rows by label and keys within a row -- asserted on the EXACT output, so the ordering itself is pinned', () => {
    // Inputs deliberately out of order on both axes: 'ctrl+k' before 'cmd+k' (within-row
    // key sort) and 'Open command palette' material before 'Archive email' material (row
    // label sort). No .sort() on the actual value here -- re-sorting the output before
    // comparing would silently discard exactly the property being tested.
    const rows = buildShortcutRows(['ctrl+k', 'e', 'cmd+k']);

    expect(rows).toEqual([
      { label: 'Archive email', keys: ['e'] },
      { label: 'Open command palette', keys: ['cmd+k', 'ctrl+k'] },
    ]);
  });

  it('falls back to the raw key string as the label for an unrecognized shortcut', () => {
    const rows = buildShortcutRows(['ctrl+alt+z']);

    expect(rows).toEqual([{ label: 'ctrl+alt+z', keys: ['ctrl+alt+z'] }]);
  });

  it('never drops an active key, even without a known label', () => {
    const rows = buildShortcutRows(['e', 'ctrl+alt+z', 'cmd+k']);
    const allKeys = rows.flatMap((r) => r.keys);

    expect(allKeys.sort()).toEqual(['cmd+k', 'ctrl+alt+z', 'e'].sort());
  });

  it('returns an empty list for no active shortcuts', () => {
    expect(buildShortcutRows([])).toEqual([]);
  });
});

describe('ShortcutHelpPanel', () => {
  beforeEach(() => {
    useShortcutHelpStore.setState({ isOpen: false });
  });

  it('renders nothing when closed', () => {
    const { container } = render(createElement(ShortcutHelpPanel));

    expect(container.firstChild).toBeNull();
  });

  it('renders shortcuts sourced live from the useKeyboard registry when open', () => {
    // Mirrors the real topology: the panel is mounted in the app shell (Layout), as a
    // SIBLING of whatever route content is registering shortcuts -- e.g. EmailClient's
    // useEmailShortcuts while /email is mounted. A co-mounted registrant is exactly what
    // production looks like.
    function Registrant() {
      useKeyboard({ e: vi.fn(), '#': vi.fn() });
      return null;
    }
    useShortcutHelpStore.setState({ isOpen: true });

    render(
      createElement('div', null, [
        createElement(Registrant, { key: 'reg' }),
        createElement(ShortcutHelpPanel, { key: 'panel' }),
      ]),
    );

    expect(screen.getByText('Archive email')).not.toBeNull();
    expect(screen.getByText('Delete email')).not.toBeNull();
  });

  it('shows a fallback message when nothing is currently registered', () => {
    useShortcutHelpStore.setState({ isOpen: true });

    render(createElement(ShortcutHelpPanel));

    expect(screen.getByText('No shortcuts are currently active on this page.')).not.toBeNull();
  });

  it('moves focus into the dialog on open (focus trap auto-focuses the close button)', () => {
    useShortcutHelpStore.setState({ isOpen: true });

    render(createElement(ShortcutHelpPanel));

    expect(document.activeElement).toBe(screen.getByLabelText('Close'));
  });

  it('restores focus to the previously-focused element on close', () => {
    const outside = document.createElement('button');
    document.body.appendChild(outside);
    outside.focus();
    render(createElement(ShortcutHelpPanel));

    act(() => {
      useShortcutHelpStore.setState({ isOpen: true });
    });
    expect(document.activeElement).not.toBe(outside);

    act(() => {
      useShortcutHelpStore.setState({ isOpen: false });
    });

    expect(document.activeElement).toBe(outside);
    document.body.removeChild(outside);
  });

  it('closes on clicking the close button', () => {
    useShortcutHelpStore.setState({ isOpen: true });
    render(createElement(ShortcutHelpPanel));

    screen.getByLabelText('Close').click();

    expect(useShortcutHelpStore.getState().isOpen).toBe(false);
  });

  it('closes on clicking the backdrop', () => {
    useShortcutHelpStore.setState({ isOpen: true });
    const { container } = render(createElement(ShortcutHelpPanel));

    (container.querySelector('[aria-hidden="true"]') as HTMLElement | null)?.click();

    expect(useShortcutHelpStore.getState().isOpen).toBe(false);
  });
});
