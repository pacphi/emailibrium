import { act } from '@testing-library/react';

/**
 * Dispatches a real `keydown` on `target` (default `document.body`, where a page-global
 * `useKeyboard` listener actually observes it), wrapped in `act()` so a state change from
 * one press (e.g. opening a panel, which changes what's registered) is flushed before a
 * subsequent press in the same test relies on it -- an unwrapped `dispatchEvent` can
 * otherwise read a stale shortcuts map from before React re-ran the registering effect.
 *
 * Pass an explicit `target` to simulate a keypress while a specific element has focus
 * (e.g. an `<input>`, to exercise useKeyboard's editable-field guard realistically).
 *
 * Lives OUTSIDE `src/` (with its own `@test-utils` alias) because it imports the
 * `@testing-library/react` devDependency -- production code must never be able to
 * reach it through an ordinary `@/` import.
 */
export function press(
  key: string,
  modifiers: Partial<KeyboardEventInit> = {},
  target: EventTarget = document.body,
): void {
  act(() => {
    target.dispatchEvent(
      new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...modifiers }),
    );
  });
}
