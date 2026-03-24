import type { ReactNode } from 'react';
import { useIsMobile, useIsTablet, useIsDesktop } from '../hooks/useMediaQuery';

export interface ResponsiveContainerProps {
  /** Content rendered on mobile viewports (max-width: 768px). */
  mobile?: ReactNode;
  /** Content rendered on tablet viewports (769px - 1024px). */
  tablet?: ReactNode;
  /** Content rendered on desktop viewports (min-width: 1025px). */
  desktop?: ReactNode;
}

/**
 * Renders different children depending on the current viewport size.
 *
 * If a slot is not provided for the active breakpoint, nothing is rendered.
 */
export function ResponsiveContainer({
  mobile,
  tablet,
  desktop,
}: ResponsiveContainerProps): ReactNode {
  const isMobile = useIsMobile();
  const isTablet = useIsTablet();
  const isDesktop = useIsDesktop();

  if (isMobile) {
    return mobile ?? null;
  }

  if (isTablet) {
    return tablet ?? null;
  }

  if (isDesktop) {
    return desktop ?? null;
  }

  // Fallback: prefer desktop content when no breakpoint matches
  return desktop ?? null;
}
