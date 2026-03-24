/**
 * Builds a consistent aria-label string from a context and action.
 *
 * @example
 * getAriaLabel('email', 'delete') // "Delete email"
 * getAriaLabel('settings', 'open') // "Open settings"
 */
export function getAriaLabel(context: string, action: string): string {
  const capitalizedAction = action.charAt(0).toUpperCase() + action.slice(1);
  return `${capitalizedAction} ${context}`;
}

/**
 * Returns a CSS selector string that matches all natively focusable
 * interactive elements. Useful for querying focusable descendants of
 * a container.
 */
export function focusableSelector(): string {
  return [
    'a[href]',
    'area[href]',
    'button:not([disabled])',
    'input:not([disabled]):not([type="hidden"])',
    'select:not([disabled])',
    'textarea:not([disabled])',
    '[tabindex]:not([tabindex="-1"])',
    '[contenteditable="true"]',
    'details > summary',
    'audio[controls]',
    'video[controls]',
  ].join(', ');
}

/**
 * Traps focus within the given container element. Returns a cleanup
 * function that removes the event listener when called.
 *
 * This is the imperative counterpart to the `useFocusTrap` hook,
 * intended for non-React contexts or manual lifecycle management.
 */
export function trapFocus(container: HTMLElement): () => void {
  const selector = focusableSelector();

  const getFocusable = (): HTMLElement[] =>
    Array.from(container.querySelectorAll<HTMLElement>(selector)).filter(
      (el) => !el.hasAttribute('disabled') && el.tabIndex >= 0,
    );

  // Focus the first element immediately
  const initialFocusable = getFocusable();
  const firstInitial = initialFocusable[0];
  if (firstInitial) {
    firstInitial.focus();
  }

  const handleKeyDown = (event: KeyboardEvent): void => {
    if (event.key !== 'Tab') {
      return;
    }

    const elements = getFocusable();
    if (elements.length === 0) {
      event.preventDefault();
      return;
    }

    const first = elements[0] as HTMLElement;
    const last = elements[elements.length - 1] as HTMLElement;

    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  container.addEventListener('keydown', handleKeyDown);

  return () => {
    container.removeEventListener('keydown', handleKeyDown);
  };
}
