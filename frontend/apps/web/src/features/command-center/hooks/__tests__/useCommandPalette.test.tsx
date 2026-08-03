// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import { useCommandPalette, useCommandPaletteStore } from '../useCommandPalette';
import { useActiveShortcuts } from '@/shared/hooks';
import { press } from '@test-utils/press';

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

  it('regression: two full useCommandPalette() consumers mounted together still toggle correctly -- the shared dispatcher fires only the newest registration, so double-mounting no longer double-toggles to a no-op (the bug CommandCenter.tsx used to have)', () => {
    render(
      <>
        <PaletteConsumer />
        <PaletteConsumer />
      </>,
    );

    press('k', { metaKey: true });

    expect(useCommandPaletteStore.getState().isOpen).toBe(true);
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

  it('closes on Escape dispatched from a focused <input> -- the palette autofocuses its search input, so this is the state every real Escape press happens in', () => {
    render(<PaletteConsumer />);
    press('k', { metaKey: true });
    expect(useCommandPaletteStore.getState().isOpen).toBe(true);
    const input = document.createElement('input');
    document.body.appendChild(input);

    press('Escape', {}, input);

    expect(useCommandPaletteStore.getState().isOpen).toBe(false);
    document.body.removeChild(input);
  });

  it('registers "escape" only while the palette is open (registry-level check)', () => {
    let activeKeys: string[] = [];
    function Probe() {
      activeKeys = useActiveShortcuts();
      return null;
    }
    render(
      <>
        <PaletteConsumer />
        <Probe />
      </>,
    );
    expect(activeKeys).not.toContain('escape');

    press('k', { metaKey: true });

    expect(activeKeys).toContain('escape');
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
