import { useCallback, useRef, useState, type ReactNode } from 'react';
import React from 'react';

interface UseAnnounceReturn {
  /** Dispatch a screen-reader announcement. */
  announce: (message: string, priority?: 'polite' | 'assertive') => void;
  /**
   * A visually-hidden live region element to render somewhere in the DOM tree.
   * Place this at the root of your app or within a layout component.
   */
  AriaLiveRegion: () => ReactNode;
}

/**
 * Creates an aria-live region that can be used to push announcements
 * to screen readers without visually affecting the page.
 *
 * The hook returns an `announce` function and a component that must
 * be rendered in the DOM.
 */
export function useAnnounce(): UseAnnounceReturn {
  const [politeMessage, setPoliteMessage] = useState('');
  const [assertiveMessage, setAssertiveMessage] = useState('');
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const announce = useCallback((message: string, priority: 'polite' | 'assertive' = 'polite') => {
    // Clear previous timeout to avoid stale clears
    if (timeoutRef.current !== null) {
      clearTimeout(timeoutRef.current);
    }

    if (priority === 'assertive') {
      // Toggle empty then set, so screen readers re-read identical messages
      setAssertiveMessage('');
      requestAnimationFrame(() => {
        setAssertiveMessage(message);
      });
    } else {
      setPoliteMessage('');
      requestAnimationFrame(() => {
        setPoliteMessage(message);
      });
    }

    // Clear after a delay so the region doesn't accumulate stale text
    timeoutRef.current = setTimeout(() => {
      setPoliteMessage('');
      setAssertiveMessage('');
    }, 7000);
  }, []);

  const AriaLiveRegion = useCallback(
    (): ReactNode =>
      React.createElement(
        React.Fragment,
        null,
        React.createElement(
          'div',
          {
            role: 'status',
            'aria-live': 'polite',
            'aria-atomic': 'true',
            style: srOnlyStyle,
          },
          politeMessage,
        ),
        React.createElement(
          'div',
          {
            role: 'alert',
            'aria-live': 'assertive',
            'aria-atomic': 'true',
            style: srOnlyStyle,
          },
          assertiveMessage,
        ),
      ),
    [politeMessage, assertiveMessage],
  );

  return { announce, AriaLiveRegion };
}

/** Standard screen-reader-only styles. */
const srOnlyStyle: React.CSSProperties = {
  position: 'absolute',
  width: '1px',
  height: '1px',
  padding: 0,
  margin: '-1px',
  overflow: 'hidden',
  clip: 'rect(0, 0, 0, 0)',
  whiteSpace: 'nowrap',
  borderWidth: 0,
};
