import { useEffect } from 'react';

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

    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [shortcuts]);
}
