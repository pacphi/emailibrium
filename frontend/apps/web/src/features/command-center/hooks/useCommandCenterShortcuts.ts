import { useMemo } from 'react';
import { useKeyboard, metaOrCtrl, type ShortcutMap } from '@/shared/hooks';
import { useShortcutHelpStore } from './useShortcutHelp';

function navigateToSettings() {
  window.location.href = '/settings';
}

/**
 * Registers the command center's remaining global shortcuts through the shared
 * useKeyboard dispatcher: Cmd+,/Ctrl+, navigates to Settings (the same navigation
 * mechanism the sidebar's Settings link already uses), ? toggles the keyboard-
 * shortcut help panel, and Escape closes it while open.
 *
 * Escape is registered here (through useKeyboard, only while the panel is open) rather
 * than as a local onKeyDown on the panel itself: nothing in the panel is focused on open
 * (unlike CommandPalette, which autofocuses its search input), so a local handler would
 * never actually receive the keydown -- it only fires for events whose target is inside
 * its own subtree, and focus never moves there. Matches useCommandPalette.ts's identical
 * `if (isOpen) map.escape = close` pattern.
 *
 * Call this from exactly one always-mounted component (CommandCenter) so each
 * shortcut is registered once -- see useCommandPalette.ts's docs for why calling a
 * useKeyboard-backed hook from two mounted components double-registers it.
 */
export function useCommandCenterShortcuts(): void {
  const isHelpOpen = useShortcutHelpStore((s) => s.isOpen);
  const toggleHelp = useShortcutHelpStore((s) => s.toggle);
  const closeHelp = useShortcutHelpStore((s) => s.close);

  const shortcuts = useMemo<ShortcutMap>(() => {
    const map: ShortcutMap = {
      ...metaOrCtrl(',', navigateToSettings),
      // '?' is Shift+/ on a standard layout -- event.shiftKey is true, so the shortcut
      // must be registered with the shift modifier for useKeyboard's exact-match to fire.
      'shift+?': toggleHelp,
    };
    if (isHelpOpen) {
      map.escape = closeHelp;
    }
    return map;
  }, [toggleHelp, isHelpOpen, closeHelp]);
  useKeyboard(shortcuts);
}
