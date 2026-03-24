export type SkeletonVariant = 'line' | 'circle' | 'rect';

export interface SkeletonProps {
  variant?: SkeletonVariant;
  /** Width in Tailwind class format (e.g., "w-full", "w-32"). Defaults to "w-full". */
  width?: string;
  /** Height in Tailwind class format (e.g., "h-4", "h-12"). Defaults based on variant. */
  height?: string;
  /** Number of skeleton instances to render. Defaults to 1. */
  count?: number;
  className?: string;
}

const DEFAULT_HEIGHT: Record<SkeletonVariant, string> = {
  line: 'h-4',
  circle: 'h-10 w-10',
  rect: 'h-24',
};

/**
 * A loading skeleton placeholder with pulse animation.
 * Supports line, circle, and rectangle variants.
 */
export function Skeleton({
  variant = 'line',
  width,
  height,
  count = 1,
  className = '',
}: SkeletonProps) {
  const resolvedHeight = height ?? DEFAULT_HEIGHT[variant];
  const resolvedWidth = width ?? (variant === 'circle' ? '' : 'w-full');
  const rounded = variant === 'circle' ? 'rounded-full' : 'rounded-md';

  const items = Array.from({ length: count }, (_, i) => i);

  return (
    <div className={`space-y-2 ${className}`} aria-hidden="true">
      {items.map((i) => (
        <div
          key={i}
          className={[
            'animate-pulse bg-gray-200 dark:bg-gray-700',
            rounded,
            resolvedWidth,
            resolvedHeight,
          ].join(' ')}
        />
      ))}
    </div>
  );
}
