import { useCallback, useEffect, useState } from 'react';

/**
 * Subscribes to a CSS media query and returns whether it currently matches.
 * Uses the matchMedia API with an event listener for live updates.
 */
export function useMediaQuery(query: string): boolean {
  const getMatches = useCallback((): boolean => {
    if (typeof window === 'undefined') {
      return false;
    }
    return window.matchMedia(query).matches;
  }, [query]);

  const [matches, setMatches] = useState<boolean>(getMatches);

  useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }

    const mediaQueryList = window.matchMedia(query);

    const handleChange = (event: MediaQueryListEvent): void => {
      setMatches(event.matches);
    };

    // Set initial value in case it changed between render and effect
    setMatches(mediaQueryList.matches);

    mediaQueryList.addEventListener('change', handleChange);

    return () => {
      mediaQueryList.removeEventListener('change', handleChange);
    };
  }, [query]);

  return matches;
}

/** Returns true when viewport width is at most 768px. */
export function useIsMobile(): boolean {
  return useMediaQuery('(max-width: 768px)');
}

/** Returns true when viewport width is between 769px and 1024px. */
export function useIsTablet(): boolean {
  return useMediaQuery('(min-width: 769px) and (max-width: 1024px)');
}

/** Returns true when viewport width is at least 1025px. */
export function useIsDesktop(): boolean {
  return useMediaQuery('(min-width: 1025px)');
}
