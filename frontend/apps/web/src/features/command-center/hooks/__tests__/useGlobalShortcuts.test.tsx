// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import { createElement } from 'react';
import { useGlobalShortcuts } from '../useGlobalShortcuts';
import { useShortcutHelpStore } from '../useShortcutHelp';
import { useActiveShortcuts } from '@/shared/hooks';
import { press } from '@test-utils/press';

// The hook navigates through the router (client-side, no page reload) -- observe the
// call instead of standing up a real router instance.
const navigateMock = vi.fn();
vi.mock('@tanstack/react-router', () => ({
  useNavigate: () => navigateMock,
}));

afterEach(() => {
  cleanup();
});

let activeKeys: string[] = [];

function mount() {
  function Harness() {
    useGlobalShortcuts();
    // Live registry readout, so tests can assert what is actually REGISTERED (not just
    // that a press was a no-op -- a registered-but-inert entry and an unregistered one
    // both no-op, and only the registry can tell them apart).
    activeKeys = useActiveShortcuts();
    return null;
  }
  return render(createElement(Harness));
}

describe('useGlobalShortcuts', () => {
  beforeEach(() => {
    useShortcutHelpStore.setState({ isOpen: false });
    navigateMock.mockClear();
  });

  it('cmd+, navigates to settings through the router', () => {
    mount();

    press(',', { metaKey: true });

    expect(navigateMock).toHaveBeenCalledWith({ to: '/settings' });
  });

  it('ctrl+, also navigates to settings', () => {
    mount();

    press(',', { ctrlKey: true });

    expect(navigateMock).toHaveBeenCalledWith({ to: '/settings' });
  });

  it('"?" toggles the shortcut help panel open when the layout produces it with shift held (US)', () => {
    mount();

    press('?', { shiftKey: true });

    expect(useShortcutHelpStore.getState().isOpen).toBe(true);
  });

  it('"?" also toggles on a layout that produces it without shift (symbol keys are layout-independent)', () => {
    mount();

    press('?');

    expect(useShortcutHelpStore.getState().isOpen).toBe(true);
  });

  it('"?" toggles the help panel closed on a second press', () => {
    mount();

    press('?', { shiftKey: true });
    expect(useShortcutHelpStore.getState().isOpen).toBe(true);

    press('?', { shiftKey: true });

    expect(useShortcutHelpStore.getState().isOpen).toBe(false);
  });

  it('registers "escape" only while the help panel is open (registry-level check)', () => {
    mount();
    expect(activeKeys).not.toContain('escape');

    press('?', { shiftKey: true });

    expect(activeKeys).toContain('escape');
  });

  it('Escape closes the help panel while open', () => {
    mount();
    press('?', { shiftKey: true });
    expect(useShortcutHelpStore.getState().isOpen).toBe(true);

    press('Escape');

    expect(useShortcutHelpStore.getState().isOpen).toBe(false);
  });

  it('is a no-op on Escape while the help panel is already closed -- because escape is NOT registered, not merely inert', () => {
    mount();

    press('Escape');

    expect(useShortcutHelpStore.getState().isOpen).toBe(false);
    expect(activeKeys).not.toContain('escape');
  });

  it('does not toggle the help panel while typing in an editable field', () => {
    mount();
    const input = document.createElement('input');
    document.body.appendChild(input);

    press('?', { shiftKey: true }, input);

    expect(useShortcutHelpStore.getState().isOpen).toBe(false);
    document.body.removeChild(input);
  });

  it('cmd+, still navigates while typing in an editable field (modifier shortcuts bypass the guard)', () => {
    mount();
    const input = document.createElement('input');
    document.body.appendChild(input);

    press(',', { metaKey: true }, input);

    expect(navigateMock).toHaveBeenCalledWith({ to: '/settings' });
    document.body.removeChild(input);
  });
});
