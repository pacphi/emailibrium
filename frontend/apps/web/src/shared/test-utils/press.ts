import { act } from '@testing-library/react';

/**
 * Dispatches a real `keydown` on `document.body` (where a page-global `useKeyboard`
 * listener actually observes it), wrapped in `act()` so a state change from one press
 * (e.g. opening a panel, which changes what's registered) is flushed before a subsequent
 * press in the same test relies on it -- an unwrapped `dispatchEvent` can otherwise read a
 * stale shortcuts map from before React re-ran the registering effect.
 */
export function press(key: string, modifiers: Partial<KeyboardEventInit> = {}): void {
  act(() => {
    document.body.dispatchEvent(
      new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...modifiers }),
    );
  });
}
