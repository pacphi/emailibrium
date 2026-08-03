// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { render, cleanup, act } from '@testing-library/react';
import { createElement } from 'react';
import { useCommandCenterShortcuts } from '../useCommandCenterShortcuts';
import { useShortcutHelpStore } from '../useShortcutHelp';

afterEach(() => {
  cleanup();
});

// act()-wrapped so a state change from one press (e.g. opening the help panel, which
// changes the `escape` registration) is flushed before a subsequent press in the same
// test relies on it -- a raw, unwrapped dispatchEvent can otherwise read a stale
// shortcuts map from before React re-ran the registering effect.
function press(key: string, modifiers: Partial<KeyboardEventInit> = {}) {
  act(() => {
    document.body.dispatchEvent(
      new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...modifiers }),
    );
  });
}

function mount() {
  function Harness() {
    useCommandCenterShortcuts();
    return null;
  }
  return render(createElement(Harness));
}

describe('useCommandCenterShortcuts', () => {
  beforeEach(() => {
    useShortcutHelpStore.setState({ isOpen: false });
  });

  // jsdom doesn't implement navigation, so a real `window.location.href = ...` assignment
  // never actually changes `location.href` in the test environment -- stub the whole
  // object so we can observe the assignment instead.
  function withMockedLocation(run: () => void) {
    const original = window.location;
    let assignedHref = '';
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: {
        get href() {
          return assignedHref;
        },
        set href(value: string) {
          assignedHref = value;
        },
      },
    });
    try {
      run();
      return assignedHref;
    } finally {
      Object.defineProperty(window, 'location', { configurable: true, value: original });
    }
  }

  it('cmd+, navigates to settings', () => {
    const assignedHref = withMockedLocation(() => {
      mount();
      press(',', { metaKey: true });
    });

    expect(assignedHref).toBe('/settings');
  });

  it('ctrl+, also navigates to settings', () => {
    const assignedHref = withMockedLocation(() => {
      mount();
      press(',', { ctrlKey: true });
    });

    expect(assignedHref).toBe('/settings');
  });

  it('shift+? toggles the shortcut help panel open', () => {
    mount();

    press('?', { shiftKey: true });

    expect(useShortcutHelpStore.getState().isOpen).toBe(true);
  });

  it('shift+? toggles the help panel closed on a second press', () => {
    mount();

    press('?', { shiftKey: true });
    expect(useShortcutHelpStore.getState().isOpen).toBe(true);

    press('?', { shiftKey: true });

    expect(useShortcutHelpStore.getState().isOpen).toBe(false);
  });

  it('bare "?" without shift does not toggle the help panel', () => {
    mount();

    press('?');

    expect(useShortcutHelpStore.getState().isOpen).toBe(false);
  });

  it('Escape closes the help panel while open', () => {
    mount();
    press('?', { shiftKey: true });
    expect(useShortcutHelpStore.getState().isOpen).toBe(true);

    press('Escape');

    expect(useShortcutHelpStore.getState().isOpen).toBe(false);
  });

  it('is a no-op on Escape while the help panel is already closed', () => {
    mount();

    press('Escape');

    expect(useShortcutHelpStore.getState().isOpen).toBe(false);
  });

  it('does not toggle the help panel while typing in an editable field', () => {
    mount();
    const input = document.createElement('input');
    document.body.appendChild(input);

    input.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: '?',
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(useShortcutHelpStore.getState().isOpen).toBe(false);
    document.body.removeChild(input);
  });
});
