import { create } from 'zustand';

interface ShortcutHelpState {
  isOpen: boolean;
  open: () => void;
  close: () => void;
  toggle: () => void;
}

/** Open/close state for the keyboard-shortcut help panel. */
export const useShortcutHelpStore = create<ShortcutHelpState>((set) => ({
  isOpen: false,
  open: () => set({ isOpen: true }),
  close: () => set({ isOpen: false }),
  toggle: () => set((state) => ({ isOpen: !state.isOpen })),
}));
