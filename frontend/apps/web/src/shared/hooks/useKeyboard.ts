import { useEffect } from 'react';
import { create } from 'zustand';

/**
 * Mapping of shortcut strings to handler functions.
 *
 * Shortcut format: modifier keys joined by `+` followed by the key name.
 * Modifiers: `ctrl`, `cmd` (Meta), `shift`, `alt`
 * Key names use `KeyboardEvent.key` values (case-insensitive).
 *
 * Examples: `"ctrl+k"`, `"cmd+shift+p"`, `"escape"`, `"ctrl+enter"`
 */
export type ShortcutMap = Record<string, () => void>;

interface ActiveShortcutsState {
  /** Reference-counted so two simultaneously-mounted registrants of the same key
   * (e.g. during a remount) don't clear each other's entry early. */
  counts: Record<string, number>;
  /** Derived from `counts` and updated only inside `register`/`unregister`, so it's a
   * stable reference between updates -- computing this fresh inside a selector instead
   * (e.g. `Object.keys(state.counts)`) returns a new array every read, which
   * useSyncExternalStore treats as a perpetual change and infinite-loops on. */
  keys: string[];
  register: (keys: string[]) => void;
  unregister: (keys: string[]) => void;
}

/**
 * A live registry of every shortcut string currently registered by a mounted
 * `useKeyboard` call, anywhere in the app. `useKeyboard` itself keeps this in sync;
 * consumers should only read it via `useActiveShortcuts`, never call `register`/
 * `unregister` directly -- that would desync it from what's actually dispatching.
 */
const useActiveShortcutsStore = create<ActiveShortcutsState>((set) => ({
  counts: {},
  keys: [],
  register: (newKeys) =>
    set((state) => {
      const counts = { ...state.counts };
      for (const key of newKeys) {
        counts[key] = (counts[key] ?? 0) + 1;
      }
      return { counts, keys: Object.keys(counts) };
    }),
  unregister: (oldKeys) =>
    set((state) => {
      const counts = { ...state.counts };
      for (const key of oldKeys) {
        const next = (counts[key] ?? 0) - 1;
        if (next <= 0) {
          delete counts[key];
        } else {
          counts[key] = next;
        }
      }
      return { counts, keys: Object.keys(counts) };
    }),
}));

/**
 * Every shortcut string currently registered by a mounted `useKeyboard` call,
 * anywhere in the app -- the ground truth for a help panel that must not drift
 * from what's actually dispatching. Order is not guaranteed.
 */
export function useActiveShortcuts(): string[] {
  return useActiveShortcutsStore((state) => state.keys);
}

interface ParsedShortcut {
  ctrl: boolean;
  meta: boolean;
  shift: boolean;
  alt: boolean;
  key: string;
}

function parseShortcut(shortcut: string): ParsedShortcut {
  const parts = shortcut.toLowerCase().split('+');
  const key = parts[parts.length - 1] ?? '';
  const modifiers = new Set(parts.slice(0, -1));

  return {
    ctrl: modifiers.has('ctrl'),
    meta: modifiers.has('cmd') || modifiers.has('meta'),
    shift: modifiers.has('shift'),
    alt: modifiers.has('alt'),
    key,
  };
}

function matchesShortcut(event: KeyboardEvent, parsed: ParsedShortcut): boolean {
  if (parsed.ctrl !== event.ctrlKey) return false;
  if (parsed.meta !== event.metaKey) return false;
  if (parsed.shift !== event.shiftKey) return false;
  if (parsed.alt !== event.altKey) return false;

  return event.key.toLowerCase() === parsed.key;
}

/**
 * Registers global keyboard shortcuts that fire the corresponding handler
 * when a matching key combination is pressed.
 *
 * Automatically calls `preventDefault` and `stopPropagation` on matched
 * events to avoid conflicts with browser defaults.
 *
 * Shortcuts are ignored when the active element is an input, textarea,
 * or contenteditable field (unless the shortcut includes a modifier key).
 */
export function useKeyboard(shortcuts: ShortcutMap): void {
  useEffect(() => {
    const parsedEntries = Object.entries(shortcuts).map(([shortcut, handler]) => ({
      parsed: parseShortcut(shortcut),
      handler,
      hasModifier:
        shortcut.toLowerCase().includes('ctrl') ||
        shortcut.toLowerCase().includes('cmd') ||
        shortcut.toLowerCase().includes('meta') ||
        shortcut.toLowerCase().includes('alt'),
    }));

    const handleKeyDown = (event: KeyboardEvent): void => {
      const target = event.target as HTMLElement | null;
      const isEditable =
        target?.tagName === 'INPUT' ||
        target?.tagName === 'TEXTAREA' ||
        target?.getAttribute('contenteditable') === 'true';

      for (const { parsed, handler, hasModifier } of parsedEntries) {
        // Skip non-modifier shortcuts when focused on editable fields
        if (isEditable && !hasModifier) {
          continue;
        }

        if (matchesShortcut(event, parsed)) {
          event.preventDefault();
          event.stopPropagation();
          handler();
          return;
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);

    const keys = Object.keys(shortcuts);
    useActiveShortcutsStore.getState().register(keys);

    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      useActiveShortcutsStore.getState().unregister(keys);
    };
  }, [shortcuts]);
}
