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
  return render(createElement(Harness));
}

function press(
  key: string,
  modifiers: Partial<
    Pick<KeyboardEventInit, 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey' | 'repeat'>
  > = {},
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

    it('dispatches only the first matching entry within one map ("cmd" and "meta" aliases both match the same event)', () => {
      // 'cmd+k' and 'meta+k' parse to the identical modifier set, so BOTH entries match a
      // single Meta+K press -- the only way two entries in one map can genuinely collide.
      // This pins the early-return: without it, one keypress would fire both handlers.
      const first = vi.fn();
      const second = vi.fn();
      mount({ 'cmd+k': first, 'meta+k': second });

      press('k', { metaKey: true });

      expect(first).toHaveBeenCalledTimes(1);
      expect(second).not.toHaveBeenCalled();
    });

    it('calls preventDefault on a match', () => {
      const handler = vi.fn();
      mount({ k: handler });

      const event = press('k');

      expect(event.defaultPrevented).toBe(true);
    });

    it('calls stopPropagation on a match', () => {
      const handler = vi.fn();
      mount({ k: handler });
      const event = new KeyboardEvent('keydown', { key: 'k', bubbles: true, cancelable: true });
      const stopSpy = vi.spyOn(event, 'stopPropagation');

      document.body.dispatchEvent(event);

      expect(stopSpy).toHaveBeenCalledTimes(1);
    });

    it('does not call preventDefault when nothing matches', () => {
      const handler = vi.fn();
      mount({ k: handler });

      const event = press('j');

      expect(event.defaultPrevented).toBe(false);
    });

    it('ignores OS key auto-repeat (holding a key fires its handler once, not repeatedly)', () => {
      const handler = vi.fn();
      mount({ e: handler });

      press('e');
      press('e', { repeat: true });
      press('e', { repeat: true });

      expect(handler).toHaveBeenCalledTimes(1);
    });
  });

  describe('symbol keys are layout-independent', () => {
    it('"#" matches when the layout produces it WITH shift held (US: Shift+3)', () => {
      const handler = vi.fn();
      mount({ '#': handler });

      press('#', { shiftKey: true });

      expect(handler).toHaveBeenCalledTimes(1);
    });

    it('"#" matches when the layout produces it WITHOUT shift (UK: dedicated # key)', () => {
      const handler = vi.fn();
      mount({ '#': handler });

      press('#');

      expect(handler).toHaveBeenCalledTimes(1);
    });

    it('"?" matches regardless of the shift flag', () => {
      const handler = vi.fn();
      mount({ '?': handler });

      press('?', { shiftKey: true });
      press('?');

      expect(handler).toHaveBeenCalledTimes(2);
    });

    it('a symbol key still requires its non-shift modifiers exactly', () => {
      const handler = vi.fn();
      mount({ '#': handler });

      press('#', { metaKey: true });

      expect(handler).not.toHaveBeenCalled();
    });

    it('letter and digit keys keep strict shift matching', () => {
      const handler = vi.fn();
      mount({ a: handler });

      press('a', { shiftKey: true });

      expect(handler).not.toHaveBeenCalled();
    });
  });

  describe('editable-field guard', () => {
    function mountWithElement(shortcuts: ShortcutMap, tagName: string) {
      const el = document.createElement(tagName);
      document.body.appendChild(el);
      mount(shortcuts);
      return el;
    }

    it('skips a non-modifier shortcut while an <input> is focused', () => {
      const handler = vi.fn();
      const input = mountWithElement({ k: handler }, 'input');

      press('k', {}, input);

      expect(handler).not.toHaveBeenCalled();
      document.body.removeChild(input);
    });

    it('skips a non-modifier shortcut while a <textarea> is focused', () => {
      const handler = vi.fn();
      const textarea = mountWithElement({ k: handler }, 'textarea');

      press('k', {}, textarea);

      expect(handler).not.toHaveBeenCalled();
      document.body.removeChild(textarea);
    });

    it('skips a non-modifier shortcut while a <select> is focused (typing in a select jumps options)', () => {
      const handler = vi.fn();
      const select = mountWithElement({ e: handler }, 'select');

      press('e', {}, select);

      expect(handler).not.toHaveBeenCalled();
      document.body.removeChild(select);
    });

    it('skips a non-modifier shortcut while a contenteditable="true" element is focused', () => {
      const handler = vi.fn();
      const div = document.createElement('div');
      div.setAttribute('contenteditable', 'true');
      document.body.appendChild(div);
      mount({ k: handler });

      press('k', {}, div);

      expect(handler).not.toHaveBeenCalled();
      document.body.removeChild(div);
    });

    it('skips a non-modifier shortcut for the empty-string contenteditable form (contenteditable="")', () => {
      const handler = vi.fn();
      const div = document.createElement('div');
      div.setAttribute('contenteditable', '');
      document.body.appendChild(div);
      mount({ k: handler });

      press('k', {}, div);

      expect(handler).not.toHaveBeenCalled();
      document.body.removeChild(div);
    });

    it('treats contenteditable="false" as NOT editable', () => {
      const handler = vi.fn();
      const div = document.createElement('div');
      div.setAttribute('contenteditable', 'false');
      document.body.appendChild(div);
      mount({ k: handler });

      press('k', {}, div);

      expect(handler).toHaveBeenCalledTimes(1);
      document.body.removeChild(div);
    });

    it('still fires a modifier shortcut while an <input> is focused', () => {
      const handler = vi.fn();
      const input = mountWithElement({ 'cmd+k': handler }, 'input');

      press('k', { metaKey: true }, input);

      expect(handler).toHaveBeenCalledTimes(1);
      document.body.removeChild(input);
    });

    it('an alt-modified shortcut also bypasses the guard (alt counts as a modifier)', () => {
      const handler = vi.fn();
      const input = mountWithElement({ 'alt+x': handler }, 'input');

      press('x', { altKey: true }, input);

      expect(handler).toHaveBeenCalledTimes(1);
      document.body.removeChild(input);
    });

    it('"escape" is exempt from the guard: it fires while an <input> is focused (it never types anything)', () => {
      const handler = vi.fn();
      const input = mountWithElement({ escape: handler }, 'input');

      press('Escape', {}, input);

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

  describe('shared dispatch across consumers (newest registration wins)', () => {
    it('fires only the newest registration when two mounted consumers register the same key', () => {
      const older = vi.fn();
      const newer = vi.fn();
      mount({ escape: older });
      mount({ escape: newer });

      press('Escape');

      expect(newer).toHaveBeenCalledTimes(1);
      expect(older).not.toHaveBeenCalled();
    });

    it('falls back to the older registration once the newer one unmounts', () => {
      const older = vi.fn();
      const newer = vi.fn();
      mount({ escape: older });
      const { unmount } = mount({ escape: newer });

      unmount();
      press('Escape');

      expect(older).toHaveBeenCalledTimes(1);
      expect(newer).not.toHaveBeenCalled();
    });

    it('consumers with disjoint keys all keep working through the shared listener', () => {
      const a = vi.fn();
      const b = vi.fn();
      mount({ 'cmd+k': a });
      mount({ e: b });

      press('k', { metaKey: true });
      press('e');

      expect(a).toHaveBeenCalledTimes(1);
      expect(b).toHaveBeenCalledTimes(1);
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

    it('keeps the shared window listener while any consumer is still mounted, removing it only with the last one', () => {
      const removeSpy = vi.spyOn(window, 'removeEventListener');
      const keydownRemovals = () =>
        removeSpy.mock.calls.filter((call) => (call[0] as string) === 'keydown').length;
      const first = mount({ k: vi.fn() });
      const second = mount({ e: vi.fn() });

      first.unmount();
      expect(keydownRemovals()).toBe(0);

      second.unmount();
      expect(keydownRemovals()).toBe(1);
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
        useKeyboard({ e: () => {}, '#': () => {} });
        return null;
      }
      render(
        createElement('div', null, [
          createElement(ConsumerA, { key: 'a' }),
          createElement(ConsumerB, { key: 'b' }),
          createElement(Reader, { key: 'r' }),
        ]),
      );

      expect(keys.sort()).toEqual(['#', 'cmd+k', 'e'].sort());
    });

    // These maps live at module scope so their identity NEVER changes across rerenders --
    // an inline object literal here would make each consumer's effect re-run (and
    // re-register) on every rerender, silently masking a broken unregister/decrement path.
    const STABLE_MAP_A: ShortcutMap = { 'cmd+k': () => {} };
    const STABLE_MAP_B: ShortcutMap = { 'cmd+k': () => {} };

    function StableConsumerA() {
      useKeyboard(STABLE_MAP_A);
      return null;
    }
    function StableConsumerB() {
      useKeyboard(STABLE_MAP_B);
      return null;
    }

    it('keeps a shared key registered while at least one consumer still holds it (reference counting, no accidental re-registration)', () => {
      let keysFromReader: string[] = [];
      function Reader() {
        keysFromReader = useActiveShortcuts();
        return null;
      }
      const { rerender } = render(
        createElement('div', null, [
          createElement(StableConsumerA, { key: 'a' }),
          createElement(StableConsumerB, { key: 'b' }),
          createElement(Reader, { key: 'r' }),
        ]),
      );
      expect(keysFromReader).toContain('cmd+k');

      // Unmount only ConsumerA. ConsumerB's map identity is stable, so its effect does
      // NOT re-run here -- if the decrement path over- or under-counted, this is where
      // the key would wrongly disappear.
      rerender(
        createElement('div', null, [
          createElement(StableConsumerB, { key: 'b' }),
          createElement(Reader, { key: 'r' }),
        ]),
      );
      expect(keysFromReader).toContain('cmd+k');

      // Unmount the last holder -- now the key must actually leave the registry.
      rerender(createElement('div', null, [createElement(Reader, { key: 'r' })]));
      expect(keysFromReader).not.toContain('cmd+k');
    });
  });
});
