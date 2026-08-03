// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import { createElement } from 'react';
import { useKeyboard, useActiveShortcuts, type ShortcutMap } from './useKeyboard';

afterEach(() => {
  cleanup();
});

function mount(shortcuts: ShortcutMap) {
  function Harness() {
    useKeyboard(shortcuts);
    return null;
  }
  render(createElement(Harness));
}

function press(
  key: string,
  modifiers: Partial<Pick<KeyboardEventInit, 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey'>> = {},
  // A real keydown's `event.target` is always an actual node (the focused element, or
  // document.body when nothing is focused) -- never `window` itself. Default to body so
  // the isEditable check in useKeyboard.ts sees a realistic target.
  target: EventTarget = document.body,
) {
  const event = new KeyboardEvent('keydown', {
    key,
    bubbles: true,
    cancelable: true,
    ...modifiers,
  });
  target.dispatchEvent(event);
  return event;
}

describe('useKeyboard', () => {
  describe('modifier matching', () => {
    it('fires the handler on an exact single-key match', () => {
      const handler = vi.fn();
      mount({ k: handler });

      press('k');

      expect(handler).toHaveBeenCalledTimes(1);
    });

    it('does not fire when the key does not match', () => {
      const handler = vi.fn();
      mount({ k: handler });

      press('j');

      expect(handler).not.toHaveBeenCalled();
    });

    it('fires on an exact modifier combination (cmd+k)', () => {
      const handler = vi.fn();
      mount({ 'cmd+k': handler });

      press('k', { metaKey: true });

      expect(handler).toHaveBeenCalledTimes(1);
    });

    it('is case-insensitive for both the modifier names and the key', () => {
      const handler = vi.fn();
      mount({ 'CMD+SHIFT+P': handler });

      press('P', { metaKey: true, shiftKey: true });

      expect(handler).toHaveBeenCalledTimes(1);
    });

    it('does not fire when an extra, unlisted modifier is also held', () => {
      const handler = vi.fn();
      mount({ 'cmd+k': handler });

      press('k', { metaKey: true, shiftKey: true });

      expect(handler).not.toHaveBeenCalled();
    });

    it('does not fire a plain-key shortcut when a modifier is held', () => {
      const handler = vi.fn();
      mount({ k: handler });

      press('k', { ctrlKey: true });

      expect(handler).not.toHaveBeenCalled();
    });

    it('treats "meta" as an alias for "cmd"', () => {
      const handler = vi.fn();
      mount({ 'meta+k': handler });

      press('k', { metaKey: true });

      expect(handler).toHaveBeenCalledTimes(1);
    });

    it('matches a three-modifier combination exactly', () => {
      const handler = vi.fn();
      mount({ 'ctrl+alt+shift+x': handler });

      press('x', { ctrlKey: true, altKey: true, shiftKey: true });

      expect(handler).toHaveBeenCalledTimes(1);
    });

    it('dispatches only the first matching entry per keydown', () => {
      const first = vi.fn();
      const second = vi.fn();
      mount({ k: first, 'cmd+k': second });

      press('k', { metaKey: true });

      expect(second).toHaveBeenCalledTimes(1);
      expect(first).not.toHaveBeenCalled();
    });

    it('calls preventDefault and stopPropagation on a match', () => {
      const handler = vi.fn();
      mount({ k: handler });

      const event = press('k');

      expect(event.defaultPrevented).toBe(true);
    });

    it('does not call preventDefault when nothing matches', () => {
      const handler = vi.fn();
      mount({ k: handler });

      const event = press('j');

      expect(event.defaultPrevented).toBe(false);
    });
  });

  describe('editable-field guard', () => {
    function mountWithInput(shortcuts: ShortcutMap) {
      const input = document.createElement('input');
      document.body.appendChild(input);
      mount(shortcuts);
      return input;
    }

    it('skips a non-modifier shortcut while an <input> is focused', () => {
      const handler = vi.fn();
      const input = mountWithInput({ k: handler });

      press('k', {}, input);

      expect(handler).not.toHaveBeenCalled();
      document.body.removeChild(input);
    });

    it('skips a non-modifier shortcut while a <textarea> is focused', () => {
      const handler = vi.fn();
      const textarea = document.createElement('textarea');
      document.body.appendChild(textarea);
      mount({ k: handler });

      press('k', {}, textarea);

      expect(handler).not.toHaveBeenCalled();
      document.body.removeChild(textarea);
    });

    it('skips a non-modifier shortcut while a contenteditable element is focused', () => {
      const handler = vi.fn();
      const div = document.createElement('div');
      div.setAttribute('contenteditable', 'true');
      document.body.appendChild(div);
      mount({ k: handler });

      press('k', {}, div);

      expect(handler).not.toHaveBeenCalled();
      document.body.removeChild(div);
    });

    it('still fires a modifier shortcut while an <input> is focused', () => {
      const handler = vi.fn();
      const input = mountWithInput({ 'cmd+k': handler });

      press('k', { metaKey: true }, input);

      expect(handler).toHaveBeenCalledTimes(1);
      document.body.removeChild(input);
    });

    it('fires a non-modifier shortcut when the target is not editable', () => {
      const handler = vi.fn();
      const div = document.createElement('div');
      document.body.appendChild(div);
      mount({ k: handler });

      press('k', {}, div);

      expect(handler).toHaveBeenCalledTimes(1);
      document.body.removeChild(div);
    });
  });

  describe('cleanup on unmount', () => {
    it('stops handling keydown after the component unmounts', () => {
      const handler = vi.fn();
      function Harness() {
        useKeyboard({ k: handler });
        return null;
      }
      const { unmount } = render(createElement(Harness));

      unmount();
      press('k');

      expect(handler).not.toHaveBeenCalled();
    });

    it('removes the window listener exactly once per registration', () => {
      const removeSpy = vi.spyOn(window, 'removeEventListener');
      function Harness() {
        useKeyboard({ k: vi.fn() });
        return null;
      }
      const { unmount } = render(createElement(Harness));

      unmount();

      const keydownRemovals = removeSpy.mock.calls.filter(
        (call) => (call[0] as string) === 'keydown',
      );
      expect(keydownRemovals).toHaveLength(1);
      removeSpy.mockRestore();
    });

    it('re-registers when the shortcuts map identity changes', () => {
      const first = vi.fn();
      const second = vi.fn();
      function Harness({ handler }: { handler: () => void }) {
        useKeyboard({ k: handler });
        return null;
      }
      const { rerender } = render(createElement(Harness, { handler: first }));
      press('k');
      expect(first).toHaveBeenCalledTimes(1);

      rerender(createElement(Harness, { handler: second }));
      press('k');

      expect(second).toHaveBeenCalledTimes(1);
      expect(first).toHaveBeenCalledTimes(1);
    });
  });

  describe('active shortcuts registry', () => {
    function HarnessWithReadout({
      shortcuts,
      onKeys,
    }: {
      shortcuts: ShortcutMap;
      onKeys: (keys: string[]) => void;
    }) {
      useKeyboard(shortcuts);
      const keys = useActiveShortcuts();
      onKeys(keys);
      return null;
    }

    it('registers its keys while mounted', () => {
      let keys: string[] = [];
      render(
        createElement(HarnessWithReadout, {
          shortcuts: { 'cmd+k': () => {} },
          onKeys: (k) => {
            keys = k;
          },
        }),
      );

      expect(keys).toContain('cmd+k');
    });

    it('unregisters its keys on unmount', () => {
      let keysFromB: string[] = [];
      function A() {
        useKeyboard({ 'cmd+k': () => {} });
        return null;
      }
      function B() {
        const keys = useActiveShortcuts();
        keysFromB = keys;
        return null;
      }
      const { unmount } = render(createElement(A));
      render(createElement(B));
      expect(keysFromB).toContain('cmd+k');

      unmount();
      render(createElement(B));

      expect(keysFromB).not.toContain('cmd+k');
    });

    it('reflects shortcuts from multiple simultaneously-mounted useKeyboard consumers', () => {
      let keys: string[] = [];
      function Reader() {
        keys = useActiveShortcuts();
        return null;
      }
      function ConsumerA() {
        useKeyboard({ 'cmd+k': () => {} });
        return null;
      }
      function ConsumerB() {
        useKeyboard({ e: () => {}, 'shift+#': () => {} });
        return null;
      }
      render(
        createElement('div', null, [
          createElement(ConsumerA, { key: 'a' }),
          createElement(ConsumerB, { key: 'b' }),
          createElement(Reader, { key: 'r' }),
        ]),
      );

      expect(keys.sort()).toEqual(['cmd+k', 'e', 'shift+#'].sort());
    });

    it('keeps a shared key registered while at least one consumer still holds it (reference counting)', () => {
      let keysFromReader: string[] = [];
      function Reader() {
        keysFromReader = useActiveShortcuts();
        return null;
      }
      function ConsumerA() {
        useKeyboard({ 'cmd+k': () => {} });
        return null;
      }
      function ConsumerB() {
        useKeyboard({ 'cmd+k': () => {} });
        return null;
      }
      const { rerender } = render(
        createElement('div', null, [
          createElement(ConsumerA, { key: 'a' }),
          createElement(ConsumerB, { key: 'b' }),
        ]),
      );
      render(createElement(Reader));
      expect(keysFromReader).toContain('cmd+k');

      // Unmount only ConsumerA -- ConsumerB still holds 'cmd+k'.
      rerender(createElement('div', null, [createElement(ConsumerB, { key: 'b' })]));
      render(createElement(Reader));

      expect(keysFromReader).toContain('cmd+k');
    });
  });
});
