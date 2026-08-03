// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
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
    expect(formatShortcutKey('shift+#')).toBe('⇧+#');
  });
});

describe('buildShortcutRows', () => {
  it('groups keys that share the same known label into one row', () => {
    const rows = buildShortcutRows(['cmd+k', 'ctrl+k']);

    expect(rows).toHaveLength(1);
    expect(rows[0]?.label).toBe('Open command palette');
    expect(rows[0]?.keys.sort()).toEqual(['cmd+k', 'ctrl+k']);
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
    function Registrant() {
      useKeyboard({ e: vi.fn(), 'shift+#': vi.fn() });
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
