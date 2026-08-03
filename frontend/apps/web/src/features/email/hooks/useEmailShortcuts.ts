import { useMemo } from 'react';
import { useKeyboard, type ShortcutMap } from '@/shared/hooks';

export type ReplyOpenMode = 'reply' | 'forward';

export interface ReplyOpenSignal {
  mode: ReplyOpenMode;
}

interface UseEmailShortcutsArgs {
  /** The currently selected/open email, if any -- `r`/`f` are no-ops without one. */
  selectedEmailId: string | null;
  onCompose: () => void;
  onOpenReply: (signal: ReplyOpenSignal) => void;
}

/**
 * Registers the email client's keyboard shortcuts (`c`/`r`/`f`) through the shared
 * useKeyboard dispatcher. `c` opens compose unconditionally; `r`/`f` open the reply
 * box in reply/forward mode for the selected email, and are no-ops with nothing selected.
 */
export function useEmailShortcuts({
  selectedEmailId,
  onCompose,
  onOpenReply,
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
    }),
    [selectedEmailId, onCompose, onOpenReply],
  );
  useKeyboard(shortcuts);
}
