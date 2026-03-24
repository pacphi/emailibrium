import React from 'react';

export interface VisuallyHiddenProps {
  children: React.ReactNode;
  /** Render as a different HTML element. Defaults to "span". */
  as?: keyof React.JSX.IntrinsicElements;
}

/**
 * Hides content visually while keeping it accessible to screen readers.
 * Uses the standard sr-only technique recommended by WebAIM.
 */
export function VisuallyHidden({
  children,
  as: Component = 'span',
}: VisuallyHiddenProps): React.ReactNode {
  return React.createElement(
    Component,
    {
      style: srOnlyStyle,
      'aria-hidden': undefined,
    },
    children,
  );
}

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
