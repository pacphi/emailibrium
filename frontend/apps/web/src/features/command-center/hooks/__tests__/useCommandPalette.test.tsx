// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import { useCommandPalette, useCommandPaletteStore } from '../useCommandPalette';
import { press } from '@/shared/test-utils/press';

// Mirrors the real app structure: CommandCenter reads only `open` from the store
// (no shortcut registration), while CommandPalette calls the full `useCommandPalette()`
// hook (isOpen/close) and is the sole shortcut registrant.
function TriggerConsumer() {
  useCommandPaletteStore((s) => s.open);
  return null;
}
function PaletteConsumer() {
  useCommandPalette();
  return null;
}

describe('useCommandPalette', () => {
  beforeEach(() => {
    useCommandPaletteStore.setState({ isOpen: false });
  });
  afterEach(() => {
    cleanup();
  });

  it('opens on Cmd+K with the real production topology mounted (trigger via store selector, palette via the full hook)', () => {
    render(
      <>
        <TriggerConsumer />
        <PaletteConsumer />
      </>,
    );

    press('k', { metaKey: true });

    expect(useCommandPaletteStore.getState().isOpen).toBe(true);
  });

  it('regression: two full useCommandPalette() consumers mounted together double-toggle to a no-op -- this is exactly the bug CommandCenter.tsx used to have before it switched to the store-only selector', () => {
    render(
      <>
        <PaletteConsumer />
        <PaletteConsumer />
      </>,
    );

    press('k', { metaKey: true });

    expect(useCommandPaletteStore.getState().isOpen).toBe(false);
  });

  it('opens on Ctrl+K', () => {
    render(<PaletteConsumer />);

    press('k', { ctrlKey: true });

    expect(useCommandPaletteStore.getState().isOpen).toBe(true);
  });

  it('closes on Escape while open', () => {
    render(<PaletteConsumer />);
    press('k', { metaKey: true });
    expect(useCommandPaletteStore.getState().isOpen).toBe(true);

    press('Escape');

    expect(useCommandPaletteStore.getState().isOpen).toBe(false);
  });

  it('is a no-op on Escape while already closed', () => {
    render(<PaletteConsumer />);

    press('Escape');

    expect(useCommandPaletteStore.getState().isOpen).toBe(false);
  });

  it('toggles closed on a second Cmd+K', () => {
    render(<PaletteConsumer />);

    press('k', { metaKey: true });
    expect(useCommandPaletteStore.getState().isOpen).toBe(true);

    press('k', { metaKey: true });
    expect(useCommandPaletteStore.getState().isOpen).toBe(false);
  });

  it('a store-only consumer (no useCommandPalette call) never registers the shortcut itself', () => {
    render(<TriggerConsumer />);

    press('k', { metaKey: true });

    expect(useCommandPaletteStore.getState().isOpen).toBe(false);
  });
});
