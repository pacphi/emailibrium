import React from 'react';

export interface SkipToContentProps {
  /** The id of the main content element to jump to. Defaults to "main-content". */
  targetId?: string;
  /** Custom label for the skip link. */
  label?: string;
}

/**
 * A "Skip to main content" link that is only visible when focused via
 * keyboard navigation. Place this as the very first child inside your
 * root layout so it is the first element in the tab order.
 */
export function SkipToContent({
  targetId = 'main-content',
  label = 'Skip to main content',
}: SkipToContentProps): React.ReactNode {
  return (
    <a
      href={`#${targetId}`}
      className={[
        'fixed left-4 top-4 z-[9999]',
        'rounded-md bg-indigo-600 px-4 py-2 text-sm font-semibold text-white',
        'opacity-0 focus:opacity-100',
        'translate-y-[-100%] focus:translate-y-0',
        'transition-all duration-150',
        'outline-none focus-visible:ring-2 focus-visible:ring-indigo-400 focus-visible:ring-offset-2',
      ].join(' ')}
    >
      {label}
    </a>
  );
}
