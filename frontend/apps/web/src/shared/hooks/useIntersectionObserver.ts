import { useEffect, useRef, useState, type RefObject } from 'react';

interface UseIntersectionObserverOptions {
  /** The element used as the viewport for checking visibility. Defaults to the browser viewport. */
  root?: Element | null;
  /** Margin around the root. Uses CSS margin syntax (e.g., "0px 0px 200px 0px"). */
  rootMargin?: string;
  /** A threshold or array of thresholds at which the callback fires. Defaults to 0. */
  threshold?: number | number[];
  /** If true, the observer disconnects after the first intersection. */
  triggerOnce?: boolean;
}

interface UseIntersectionObserverReturn {
  /** Ref to attach to the target element. */
  ref: RefObject<HTMLElement | null>;
  /** Whether the target element is currently intersecting the viewport. */
  isIntersecting: boolean;
  /** The most recent IntersectionObserverEntry, if available. */
  entry: IntersectionObserverEntry | null;
}

/**
 * Observes when an element enters or leaves the viewport using the
 * IntersectionObserver API. Useful for lazy loading, infinite scroll,
 * and triggering animations on scroll.
 */
export function useIntersectionObserver(
  options?: UseIntersectionObserverOptions,
): UseIntersectionObserverReturn {
  const ref = useRef<HTMLElement | null>(null);
  const [entry, setEntry] = useState<IntersectionObserverEntry | null>(null);
  const [isIntersecting, setIsIntersecting] = useState(false);
  const triggered = useRef(false);

  useEffect(() => {
    const element = ref.current;

    if (!element) return;
    if (options?.triggerOnce && triggered.current) return;

    const observer = new IntersectionObserver(
      ([observerEntry]) => {
        if (!observerEntry) return;

        setEntry(observerEntry);
        setIsIntersecting(observerEntry.isIntersecting);

        if (observerEntry.isIntersecting && options?.triggerOnce) {
          triggered.current = true;
          observer.disconnect();
        }
      },
      {
        root: options?.root ?? null,
        rootMargin: options?.rootMargin ?? '0px',
        threshold: options?.threshold ?? 0,
      },
    );

    observer.observe(element);

    return () => {
      observer.disconnect();
    };
  }, [options?.root, options?.rootMargin, options?.threshold, options?.triggerOnce]);

  return { ref, isIntersecting, entry };
}
