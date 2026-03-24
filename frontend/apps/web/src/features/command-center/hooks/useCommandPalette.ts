import { create } from 'zustand';
import { useEffect } from 'react';

interface CommandPaletteState {
  isOpen: boolean;
  open: () => void;
  close: () => void;
  toggle: () => void;
}

export const useCommandPaletteStore = create<CommandPaletteState>((set) => ({
  isOpen: false,
  open: () => set({ isOpen: true }),
  close: () => set({ isOpen: false }),
  toggle: () => set((state) => ({ isOpen: !state.isOpen })),
}));

/**
 * Hook that manages command palette open/close state and registers
 * the global Cmd+K / Ctrl+K keyboard shortcut.
 */
export function useCommandPalette() {
  const { isOpen, open, close, toggle } = useCommandPaletteStore();

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key === 'k') {
        event.preventDefault();
        toggle();
      }

      if (event.key === 'Escape' && isOpen) {
        event.preventDefault();
        close();
      }
    }

    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, close, toggle]);

  return { isOpen, open, close, toggle };
}
