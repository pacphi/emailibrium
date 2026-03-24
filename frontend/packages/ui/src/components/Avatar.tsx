export interface AvatarProps {
  name: string;
  size?: 'sm' | 'md' | 'lg';
  className?: string;
}

const SIZE_CLASSES: Record<NonNullable<AvatarProps['size']>, string> = {
  sm: 'h-8 w-8 text-xs',
  md: 'h-10 w-10 text-sm',
  lg: 'h-14 w-14 text-lg',
};

const COLORS = [
  'bg-red-500',
  'bg-orange-500',
  'bg-amber-500',
  'bg-emerald-500',
  'bg-teal-500',
  'bg-cyan-500',
  'bg-blue-500',
  'bg-indigo-500',
  'bg-violet-500',
  'bg-purple-500',
  'bg-pink-500',
  'bg-rose-500',
];

/**
 * Returns initials from a full name (up to two characters).
 */
function getInitials(name: string): string {
  const parts = name.trim().split(/\s+/);
  if (parts.length === 0) return '?';
  if (parts.length === 1) return (parts[0]?.[0] ?? '?').toUpperCase();
  return `${parts[0]?.[0] ?? ''}${parts[parts.length - 1]?.[0] ?? ''}`.toUpperCase();
}

/**
 * Deterministic color based on string hash.
 */
function hashColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash);
  }
  const index = Math.abs(hash) % COLORS.length;
  return COLORS[index] ?? COLORS[0]!;
}

/**
 * Displays user initials inside a colored circle.
 * The color is deterministically derived from the name
 * so the same user always gets the same color.
 */
export function Avatar({ name, size = 'md', className = '' }: AvatarProps) {
  const initials = getInitials(name);
  const bgColor = hashColor(name);

  return (
    <div
      className={[
        'inline-flex flex-shrink-0 items-center justify-center rounded-full font-medium text-white',
        bgColor,
        SIZE_CLASSES[size],
        className,
      ].join(' ')}
      role="img"
      aria-label={name}
    >
      {initials}
    </div>
  );
}
