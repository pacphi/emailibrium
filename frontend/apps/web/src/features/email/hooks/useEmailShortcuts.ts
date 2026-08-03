import { useMemo } from 'react';
import { useKeyboard, metaOrCtrl, type ShortcutMap } from '@/shared/hooks';

export type ReplyOpenMode = 'reply' | 'forward';

export interface ReplyOpenSignal {
  mode: ReplyOpenMode;
}

interface UseEmailShortcutsArgs {
  /** When false (e.g. while the Compose or Move modal, or a confirmation dialog, is
   * open), NOTHING is registered: no shortcut can fire behind the overlay, and the
   * help panel's live registry stops listing these keys until it closes. */
  enabled?: boolean;
  /** The currently selected/open email, if any -- `r`/`f` are no-ops without one. */
  selectedEmailId: string | null;
  onCompose: () => void;
  onOpenReply: (signal: ReplyOpenSignal) => void;
  /** Omit to leave `e` unregistered entirely -- the trash/spam views do this because
   * their action bar offers no Archive, and an unregistered key (unlike a registered
   * no-op) also keeps the help panel truthful for those views. */
  onArchive?: () => void;
  /** What Delete means for the current view: the inbox's move-to-trash, or the trash
   * view's confirmation-gated permanent delete. */
  onDelete: () => void;
  /** Selects every VISIBLE email in the current view. */
  onSelectAll: () => void;
}

/**
 * Registers the email client's keyboard shortcuts through the shared useKeyboard
 * dispatcher. `c` opens compose unconditionally; `r`/`f` open the reply box in
 * reply/forward mode for the selected email (no-op with nothing selected); `e`/`#`
 * archive/delete via the handlers the current view's action bar uses (so they inherit
 * both its no-op-with-nothing-selected semantics and its per-view rules); `cmd+shift+a`
 * (and `ctrl+shift+a` for parity, matching cmd+k/ctrl+k) selects all visible.
 *
 * `#` is registered without a shift modifier: `event.key` is already the final
 * character, and useKeyboard treats symbol keys as layout-independent (US layouts
 * produce `#` with Shift held, others with a dedicated key).
 */
export function useEmailShortcuts({
  enabled = true,
  selectedEmailId,
  onCompose,
  onOpenReply,
  onArchive,
  onDelete,
  onSelectAll,
}: UseEmailShortcutsArgs): void {
  const shortcuts = useMemo<ShortcutMap>(() => {
    if (!enabled) return {};
    const map: ShortcutMap = {
      c: onCompose,
      r: () => {
        if (selectedEmailId) onOpenReply({ mode: 'reply' });
      },
      f: () => {
        if (selectedEmailId) onOpenReply({ mode: 'forward' });
      },
    };
    if (onArchive) {
      map.e = onArchive;
    }
    map['#'] = onDelete;
    Object.assign(map, metaOrCtrl('shift+a', onSelectAll));
    return map;
  }, [enabled, selectedEmailId, onCompose, onOpenReply, onArchive, onDelete, onSelectAll]);
  useKeyboard(shortcuts);
}
