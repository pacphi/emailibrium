import { useMemo } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { useKeyboard, metaOrCtrl, type ShortcutMap } from '@/shared/hooks';
import { useShortcutHelpStore } from './useShortcutHelp';

/**
 * Registers the app-shell-wide shortcuts through the shared useKeyboard dispatcher:
 * Cmd+,/Ctrl+, navigates to Settings (client-side, via the router -- no page reload),
 * ? toggles the keyboard-shortcut help panel, and Escape closes it while open.
 *
 * `?` is registered without a shift modifier: `event.key` is already the final
 * character, and useKeyboard treats symbol keys as layout-independent (US layouts
 * produce `?` with Shift held, others without).
 *
 * Escape is registered here (through useKeyboard, only while the panel is open) rather
 * than as a local onKeyDown on the panel itself, matching useCommandPalette.ts's
 * identical `if (isOpen) map.escape = close` pattern -- the shared dispatcher gives the
 * most recently opened overlay priority when several register `escape` at once.
 *
 * Call this from exactly one always-mounted component (the app shell, `Layout`) so each
 * shortcut is registered once -- see useCommandPalette.ts's docs for why a
 * useKeyboard-backed hook should have a single mounted registrant.
 */
export function useGlobalShortcuts(): void {
  const navigate = useNavigate();
  const isHelpOpen = useShortcutHelpStore((s) => s.isOpen);
  const toggleHelp = useShortcutHelpStore((s) => s.toggle);
  const closeHelp = useShortcutHelpStore((s) => s.close);

  const shortcuts = useMemo<ShortcutMap>(() => {
    const map: ShortcutMap = {
      ...metaOrCtrl(',', () => void navigate({ to: '/settings' })),
      '?': toggleHelp,
    };
    if (isHelpOpen) {
      map.escape = closeHelp;
    }
    return map;
  }, [navigate, toggleHelp, isHelpOpen, closeHelp]);
  useKeyboard(shortcuts);
}
