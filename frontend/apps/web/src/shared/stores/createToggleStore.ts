import { create } from 'zustand';
import type { UseBoundStore, StoreApi } from 'zustand';

export interface ToggleState {
  isOpen: boolean;
  open: () => void;
  close: () => void;
  toggle: () => void;
}

/** A tiny Zustand store for isOpen/open/close/toggle state -- the shape shared by every
 * modal/panel in this codebase that's driven by a keyboard shortcut (command palette,
 * shortcut help panel, ...). */
export function createToggleStore(): UseBoundStore<StoreApi<ToggleState>> {
  return create<ToggleState>((set) => ({
    isOpen: false,
    open: () => set({ isOpen: true }),
    close: () => set({ isOpen: false }),
    toggle: () => set((state) => ({ isOpen: !state.isOpen })),
  }));
}
