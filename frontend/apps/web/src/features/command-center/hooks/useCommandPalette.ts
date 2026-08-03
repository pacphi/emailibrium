import { useMemo } from 'react';
import { useKeyboard, metaOrCtrl, type ShortcutMap } from '@/shared/hooks';
import { createToggleStore } from '@/shared/stores/createToggleStore';

export const useCommandPaletteStore = createToggleStore();

/**
 * Hook that manages command palette open/close state and registers
 * the global Cmd+K / Ctrl+K keyboard shortcut (plus Escape-to-close) through
 * the shared `useKeyboard` dispatcher.
 *
 * Call this from exactly one always-mounted component (`CommandPalette`) so the
 * shortcut is registered once. A second call site should read `useCommandPaletteStore`
 * directly instead (e.g. a trigger button only needs `open`) -- calling this hook from
 * two mounted components double-registers the shortcut, which cancels itself out on
 * every keypress (two `toggle()` calls net to a no-op).
 */
export function useCommandPalette() {
  const { isOpen, open, close, toggle } = useCommandPaletteStore();

  const shortcuts = useMemo<ShortcutMap>(() => {
    const map: ShortcutMap = metaOrCtrl('k', toggle);
    // Only register `escape` while open. useKeyboard calls preventDefault/stopPropagation on
    // any matched key, so an always-registered entry would swallow every bare Escape press on
    // this route even while closed -- the original raw listener only acted (and preventDefault'd)
    // when `isOpen`, and this preserves that exactly.
    if (isOpen) {
      map.escape = close;
    }
    return map;
  }, [toggle, isOpen, close]);
  useKeyboard(shortcuts);

  return { isOpen, open, close, toggle };
}
