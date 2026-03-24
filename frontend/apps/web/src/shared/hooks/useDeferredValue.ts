import { useDeferredValue as reactUseDeferredValue, useEffect, useState } from 'react';

const HAS_NATIVE_DEFERRED_VALUE = typeof reactUseDeferredValue === 'function';

/**
 * Wrapper around React.useDeferredValue that provides a debounce-based
 * fallback for environments where useDeferredValue is not available
 * (e.g., older React versions or during SSR).
 *
 * The availability check is evaluated once at module load time so the
 * hook call pattern is stable across renders.
 *
 * @param value - The value to defer.
 * @param delay - Fallback debounce delay in milliseconds. Defaults to 200.
 * @returns The deferred value.
 */
export function useDeferredValue<T>(value: T, delay = 200): T {
  if (HAS_NATIVE_DEFERRED_VALUE) {
    // Safe: the branch is determined at module load and never changes.
    // eslint-disable-next-line react-hooks/rules-of-hooks
    return reactUseDeferredValue(value);
  }

  // eslint-disable-next-line react-hooks/rules-of-hooks
  return useDebouncedFallback(value, delay);
}

function useDebouncedFallback<T>(value: T, delay: number): T {
  const [deferredValue, setDeferredValue] = useState(value);

  useEffect(() => {
    const timer = setTimeout(() => {
      setDeferredValue(value);
    }, delay);

    return () => {
      clearTimeout(timer);
    };
  }, [value, delay]);

  return deferredValue;
}
