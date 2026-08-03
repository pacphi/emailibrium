import { useEffect } from 'react';
import { create } from 'zustand';
import { useShallow } from 'zustand/react/shallow';

/**
 * Mapping of shortcut strings to handler functions.
 *
 * Shortcut format: modifier keys joined by `+` followed by the key name.
 * Modifiers: `ctrl`, `cmd` (Meta), `shift`, `alt`
 * Key names use `KeyboardEvent.key` values (case-insensitive).
 *
 * Examples: `"ctrl+k"`, `"cmd+shift+p"`, `"escape"`, `"ctrl+enter"`
 *
 * Symbol keys (`"#"`, `"?"`, ...) are registered WITHOUT a shift modifier:
 * `event.key` is already the layout-resolved character, and whether Shift was
 * involved in producing it varies by keyboard layout (US `#` is Shift+3; UK `#`
 * is its own key), so the dispatcher ignores the shift flag for these keys.
 */
export type ShortcutMap = Record<string, () => void>;

interface ActiveShortcutsState {
  /** Reference-counted so two simultaneously-mounted registrants of the same key
   * (e.g. during a remount) don't clear each other's entry early. */
  counts: Record<string, number>;
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
  register: (newKeys) =>
    set((state) => {
      const counts = { ...state.counts };
      for (const key of newKeys) {
        counts[key] = (counts[key] ?? 0) + 1;
      }
      return { counts };
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
      return { counts };
    }),
}));

/**
 * Every shortcut string currently registered by a mounted `useKeyboard` call,
 * anywhere in the app -- the ground truth for a help panel that must not drift
 * from what's actually dispatching. Order is not guaranteed.
 *
 * Wrapped in `useShallow`: `Object.keys(state.counts)` builds a new array every read,
 * which would otherwise make useSyncExternalStore see a perpetual change and infinite-loop;
 * useShallow memoizes by shallow-comparing the array's contents instead.
 */
export function useActiveShortcuts(): string[] {
  return useActiveShortcutsStore(useShallow((state) => Object.keys(state.counts)));
}

interface ParsedShortcut {
  ctrl: boolean;
  meta: boolean;
  shift: boolean;
  alt: boolean;
  key: string;
}

/** Splits a shortcut string into its modifier tokens (in written order) and final key,
 * e.g. "cmd+shift+a" -> `{ modifierTokens: ["cmd", "shift"], key: "a" }`. Exported so
 * anything that needs to *display* a shortcut (not just match it) can reuse the same
 * grammar instead of re-deriving it. */
export function splitShortcut(shortcut: string): { modifierTokens: string[]; key: string } {
  const parts = shortcut.split('+');
  const key = parts[parts.length - 1] ?? '';
  return { modifierTokens: parts.slice(0, -1), key };
}

function parseShortcut(shortcut: string): ParsedShortcut {
  const { modifierTokens, key } = splitShortcut(shortcut.toLowerCase());
  const modifiers = new Set(modifierTokens);

  return {
    ctrl: modifiers.has('ctrl'),
    meta: modifiers.has('cmd') || modifiers.has('meta'),
    shift: modifiers.has('shift'),
    alt: modifiers.has('alt'),
    key,
  };
}

/** Both entries a Cmd+K-style shortcut needs for Mac/Windows-Linux parity, e.g.
 * `metaOrCtrl('k', toggle)` -> `{'cmd+k': toggle, 'ctrl+k': toggle}`. */
export function metaOrCtrl(key: string, handler: () => void): ShortcutMap {
  return { [`cmd+${key}`]: handler, [`ctrl+${key}`]: handler };
}

/** A single printable character that isn't a letter or digit -- `#`, `?`, `,`, ... */
function isSymbolKey(key: string): boolean {
  return key.length === 1 && !/[a-z0-9]/.test(key);
}

function matchesShortcut(event: KeyboardEvent, parsed: ParsedShortcut): boolean {
  if (parsed.ctrl !== event.ctrlKey) return false;
  if (parsed.meta !== event.metaKey) return false;
  if (parsed.alt !== event.altKey) return false;
  // For symbol keys, `event.key` is already the final layout-resolved character and the
  // shift flag only reflects the *physical layout* that produced it (US `#` sets shiftKey,
  // a UK dedicated `#` key doesn't) -- so shift carries no signal and either state matches.
  if (!isSymbolKey(parsed.key) && parsed.shift !== event.shiftKey) return false;

  return event.key.toLowerCase() === parsed.key;
}

function isEditableTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el?.tagName) return false;
  if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT') return true;
  // `isContentEditable` covers every editable form (contenteditable="", "plaintext-only",
  // descendants of an editable host); jsdom doesn't implement the property, so fall back
  // to the attribute -- any value except "false" means editable.
  if (el.isContentEditable) return true;
  const attr = el.getAttribute?.('contenteditable');
  return attr != null && attr !== 'false';
}

interface RegistrationEntry {
  parsed: ParsedShortcut;
  handler: () => void;
  hasModifier: boolean;
}

/** Every mounted `useKeyboard` call's parsed entries, oldest registration first.
 * One shared window listener dispatches over this stack -- newest first -- so a key
 * registered by two consumers at once (e.g. `escape` from two stacked overlays) fires
 * exactly one handler: the most recently (re-)registered one, which for the
 * conditional-registration pattern this codebase uses is the overlay opened last. */
const registrationStack: RegistrationEntry[][] = [];

function handleKeyDown(event: KeyboardEvent): void {
  // OS key auto-repeat must not re-fire toggles or destructive actions.
  if (event.repeat) return;

  const isEditable = isEditableTarget(event.target);

  for (let i = registrationStack.length - 1; i >= 0; i--) {
    for (const { parsed, handler, hasModifier } of registrationStack[i]!) {
      // Skip shortcuts that would conflict with typing while an editable field is
      // focused. `escape` is exempt: it never inserts text, and overlays depend on it
      // firing while their own autofocused input holds focus.
      if (isEditable && !hasModifier && parsed.key !== 'escape') {
        continue;
      }

      if (matchesShortcut(event, parsed)) {
        event.preventDefault();
        event.stopPropagation();
        handler();
        return;
      }
    }
  }
}

/**
 * Registers global keyboard shortcuts that fire the corresponding handler
 * when a matching key combination is pressed.
 *
 * All consumers share ONE window keydown listener. When several mounted consumers
 * register the same key, only the newest registration's handler fires (see
 * `registrationStack`) -- there is no double-dispatch.
 *
 * Automatically calls `preventDefault` and `stopPropagation` on matched
 * events to avoid conflicts with browser defaults, and ignores OS key
 * auto-repeat.
 *
 * Shortcuts are ignored while an input, textarea, select, or contenteditable
 * element is focused, unless the shortcut includes a modifier key (ctrl/cmd/alt)
 * or is `escape` (which never types anything).
 */
export function useKeyboard(shortcuts: ShortcutMap): void {
  useEffect(() => {
    const entries: RegistrationEntry[] = Object.entries(shortcuts).map(([shortcut, handler]) => {
      const parsed = parseShortcut(shortcut);
      return { parsed, handler, hasModifier: parsed.ctrl || parsed.meta || parsed.alt };
    });

    if (registrationStack.length === 0) {
      window.addEventListener('keydown', handleKeyDown);
    }
    registrationStack.push(entries);

    const keys = Object.keys(shortcuts);
    useActiveShortcutsStore.getState().register(keys);

    return () => {
      const index = registrationStack.indexOf(entries);
      if (index !== -1) {
        registrationStack.splice(index, 1);
      }
      if (registrationStack.length === 0) {
        window.removeEventListener('keydown', handleKeyDown);
      }
      useActiveShortcutsStore.getState().unregister(keys);
    };
  }, [shortcuts]);
}
