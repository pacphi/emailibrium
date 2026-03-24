import type { ReactNode } from 'react';

export interface CardProps {
  children: ReactNode;
  header?: ReactNode;
  footer?: ReactNode;
  className?: string;
}

/**
 * A container card with optional header and footer sections.
 * Uses a subtle border and shadow for visual separation.
 */
export function Card({ children, header, footer, className = '' }: CardProps) {
  return (
    <div
      className={[
        'rounded-lg border border-gray-200 bg-white shadow-sm dark:border-gray-700 dark:bg-gray-800',
        className,
      ].join(' ')}
    >
      {header && (
        <div className="border-b border-gray-200 px-4 py-3 dark:border-gray-700">
          {header}
        </div>
      )}
      <div className="px-4 py-4">{children}</div>
      {footer && (
        <div className="border-t border-gray-200 px-4 py-3 dark:border-gray-700">
          {footer}
        </div>
      )}
    </div>
  );
}
