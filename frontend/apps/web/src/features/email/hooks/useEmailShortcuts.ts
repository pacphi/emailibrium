import { useMemo } from 'react';
import { useKeyboard, metaOrCtrl, type ShortcutMap } from '@/shared/hooks';

export type ReplyOpenMode = 'reply' | 'forward';

export interface ReplyOpenSignal {
  mode: ReplyOpenMode;
}

interface UseEmailShortcutsArgs {
  /** The currently selected/open email, if any -- `r`/`f` are no-ops without one. */
  selectedEmailId: string | null;
  onCompose: () => void;
  onOpenReply: (signal: ReplyOpenSignal) => void;
  /** Same function the thread action bar's Archive button calls -- already a no-op with
   * nothing selected/checked, so no extra guard is needed here. */
  onArchive: () => void;
  /** Same function the thread action bar's Delete button calls -- already a no-op with
   * nothing selected/checked. */
  onDelete: () => void;
  /** Selects every email in the current filtered/grouped view. */
  onSelectAll: () => void;
}

/**
 * Registers the email client's keyboard shortcuts through the shared useKeyboard
 * dispatcher. `c` opens compose unconditionally; `r`/`f` open the reply box in
 * reply/forward mode for the selected email (no-op with nothing selected); `e`/`shift+#`
 * archive/delete (reusing the same thread-action-bar functions the click UI calls, so they
 * inherit that behavior's no-op-with-nothing-selected-or-checked semantics); `cmd+shift+a`
 * (and `ctrl+shift+a` for parity, matching cmd+k/ctrl+k) selects all.
 */
export function useEmailShortcuts({
  selectedEmailId,
  onCompose,
  onOpenReply,
  onArchive,
  onDelete,
  onSelectAll,
}: UseEmailShortcutsArgs): void {
  const shortcuts = useMemo<ShortcutMap>(
    () => ({
      c: onCompose,
      r: () => {
        if (selectedEmailId) onOpenReply({ mode: 'reply' });
      },
      f: () => {
        if (selectedEmailId) onOpenReply({ mode: 'forward' });
      },
      e: onArchive,
      // '#' is Shift+3 on a standard layout -- event.shiftKey is true, so the shortcut
      // must be registered with the shift modifier for useKeyboard's exact-match to fire.
      'shift+#': onDelete,
      ...metaOrCtrl('shift+a', onSelectAll),
    }),
    [selectedEmailId, onCompose, onOpenReply, onArchive, onDelete, onSelectAll],
  );
  useKeyboard(shortcuts);
}
