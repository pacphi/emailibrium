import type { ReactNode } from 'react';

export interface EmptyStateProps {
  icon?: ReactNode;
  title: string;
  description?: string;
  action?: ReactNode;
  className?: string;
}

/**
 * A placeholder component for empty lists or search results.
 * Displays an optional icon, title, description, and action button.
 */
export function EmptyState({ icon, title, description, action, className = '' }: EmptyStateProps) {
  return (
    <div
      className={['flex flex-col items-center justify-center py-12 text-center', className].join(
        ' ',
      )}
    >
      {icon && (
        <div className="mb-4 text-gray-300 dark:text-gray-600" aria-hidden="true">
          {icon}
        </div>
      )}
      <h3 className="text-sm font-semibold text-gray-900 dark:text-white">{title}</h3>
      {description && (
        <p className="mt-1 max-w-sm text-sm text-gray-500 dark:text-gray-400">{description}</p>
      )}
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
}
