import { useCallback, useEffect, useRef } from 'react';
import { focusableSelector } from '../utils/a11y';

/**
 * Traps keyboard focus within a container element. Useful for modals and
 * dialogs to keep focus from escaping to content behind the overlay.
 *
 * - Cycles Tab / Shift+Tab within the container
 * - Auto-focuses the first focusable element on mount
 * - Restores focus to the previously-focused element on unmount
 */
export function useFocusTrap<T extends HTMLElement = HTMLElement>() {
  const containerRef = useRef<T | null>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);

  const getFocusableElements = useCallback((): HTMLElement[] => {
    if (!containerRef.current) {
      return [];
    }
    return Array.from(
      containerRef.current.querySelectorAll<HTMLElement>(focusableSelector()),
    ).filter((el) => !el.hasAttribute('disabled') && el.tabIndex >= 0);
  }, []);

  useEffect(() => {
    // Save the element that had focus before the trap was activated
    previousFocusRef.current = document.activeElement as HTMLElement | null;

    const container = containerRef.current;
    if (!container) {
      return;
    }

    // Auto-focus the first focusable element inside the container
    const focusable = getFocusableElements();
    const firstFocusable = focusable[0];
    if (firstFocusable) {
      firstFocusable.focus();
    }

    const handleKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== 'Tab') {
        return;
      }

      const elements = getFocusableElements();
      if (elements.length === 0) {
        event.preventDefault();
        return;
      }

      const firstElement = elements[0] as HTMLElement;
      const lastElement = elements[elements.length - 1] as HTMLElement;

      if (event.shiftKey) {
        // Shift+Tab: if focus is on first element, wrap to last
        if (document.activeElement === firstElement) {
          event.preventDefault();
          lastElement.focus();
        }
      } else {
        // Tab: if focus is on last element, wrap to first
        if (document.activeElement === lastElement) {
          event.preventDefault();
          firstElement.focus();
        }
      }
    };

    container.addEventListener('keydown', handleKeyDown);

    return () => {
      container.removeEventListener('keydown', handleKeyDown);

      // Restore focus to the element that was focused before the trap
      if (previousFocusRef.current && typeof previousFocusRef.current.focus === 'function') {
        previousFocusRef.current.focus();
      }
    };
  }, [getFocusableElements]);

  return containerRef;
}
