import { describe, it, expect } from 'vitest';
import { createToggleStore } from '../createToggleStore';

describe('createToggleStore', () => {
  it('starts closed', () => {
    const useStore = createToggleStore();
    expect(useStore.getState().isOpen).toBe(false);
  });

  it('open() sets isOpen to true', () => {
    const useStore = createToggleStore();
    useStore.getState().open();
    expect(useStore.getState().isOpen).toBe(true);
  });

  it('close() sets isOpen to false', () => {
    const useStore = createToggleStore();
    useStore.getState().open();
    useStore.getState().close();
    expect(useStore.getState().isOpen).toBe(false);
  });

  it('toggle() flips isOpen', () => {
    const useStore = createToggleStore();
    useStore.getState().toggle();
    expect(useStore.getState().isOpen).toBe(true);
    useStore.getState().toggle();
    expect(useStore.getState().isOpen).toBe(false);
  });

  it('returns independent store instances', () => {
    const storeA = createToggleStore();
    const storeB = createToggleStore();
    storeA.getState().open();
    expect(storeA.getState().isOpen).toBe(true);
    expect(storeB.getState().isOpen).toBe(false);
  });
});
