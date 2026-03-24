import { useEffect, useState } from 'react';

/** Tailwind CSS default breakpoint names. */
export type Breakpoint = 'sm' | 'md' | 'lg' | 'xl' | '2xl';

interface BreakpointConfig {
  name: Breakpoint;
  minWidth: number;
}

/**
 * Tailwind default breakpoints ordered from largest to smallest so the
 * first match wins.
 */
const BREAKPOINTS: BreakpointConfig[] = [
  { name: '2xl', minWidth: 1536 },
  { name: 'xl', minWidth: 1280 },
  { name: 'lg', minWidth: 1024 },
  { name: 'md', minWidth: 768 },
  { name: 'sm', minWidth: 640 },
];

function resolveBreakpoint(): Breakpoint {
  if (typeof window === 'undefined') {
    return 'sm';
  }

  const width = window.innerWidth;

  for (const bp of BREAKPOINTS) {
    if (width >= bp.minWidth) {
      return bp.name;
    }
  }

  return 'sm';
}

/**
 * Returns the current Tailwind breakpoint name based on viewport width.
 * Updates reactively when the window resizes.
 */
export function useBreakpoint(): Breakpoint {
  const [breakpoint, setBreakpoint] = useState<Breakpoint>(resolveBreakpoint);

  useEffect(() => {
    const handleResize = (): void => {
      setBreakpoint(resolveBreakpoint());
    };

    window.addEventListener('resize', handleResize);
    return () => {
      window.removeEventListener('resize', handleResize);
    };
  }, []);

  return breakpoint;
}
