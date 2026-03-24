/**
 * Measures the wall-clock execution time of a synchronous function.
 *
 * @param label - A descriptive label printed to the console.
 * @param fn - The function to measure.
 * @returns The elapsed time in milliseconds.
 */
export function measurePerformance(label: string, fn: () => void): number {
  const markName = `perf-${label}`;
  performance.mark(markName);
  const start = performance.now();
  fn();
  const elapsed = performance.now() - start;

  return elapsed;
}

interface WebVitalMetric {
  name: string;
  value: number;
}

/**
 * Reports Core Web Vitals (LCP, FID, CLS) by setting up PerformanceObserver
 * instances. The provided callback is invoked for each metric as it becomes
 * available.
 *
 * Safe to call in environments where PerformanceObserver is not supported;
 * the function becomes a no-op in that case.
 */
export function reportWebVitals(callback: (metric: WebVitalMetric) => void): void {
  if (typeof PerformanceObserver === 'undefined') return;

  // Largest Contentful Paint
  try {
    const lcpObserver = new PerformanceObserver((list) => {
      const entries = list.getEntries();
      const lastEntry = entries[entries.length - 1];
      if (lastEntry) {
        callback({ name: 'LCP', value: lastEntry.startTime });
      }
    });
    lcpObserver.observe({ type: 'largest-contentful-paint', buffered: true });
  } catch {
    // Observer type not supported — skip
  }

  // First Input Delay
  try {
    const fidObserver = new PerformanceObserver((list) => {
      const entries = list.getEntries();
      const firstEntry = entries[0] as PerformanceEventTiming | undefined;
      if (firstEntry) {
        callback({ name: 'FID', value: firstEntry.processingStart - firstEntry.startTime });
      }
    });
    fidObserver.observe({ type: 'first-input', buffered: true });
  } catch {
    // Observer type not supported — skip
  }

  // Cumulative Layout Shift
  try {
    let clsValue = 0;
    const clsObserver = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        const layoutShiftEntry = entry as PerformanceEntry & {
          hadRecentInput?: boolean;
          value?: number;
        };
        if (!layoutShiftEntry.hadRecentInput && layoutShiftEntry.value != null) {
          clsValue += layoutShiftEntry.value;
          callback({ name: 'CLS', value: clsValue });
        }
      }
    });
    clsObserver.observe({ type: 'layout-shift', buffered: true });
  } catch {
    // Observer type not supported — skip
  }
}

const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

/**
 * Formats a byte count into a human-readable string (e.g., 1024 -> "1 KB").
 */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';

  const isNegative = bytes < 0;
  let absoluteBytes = Math.abs(bytes);
  let unitIndex = 0;

  while (absoluteBytes >= 1024 && unitIndex < BYTE_UNITS.length - 1) {
    absoluteBytes /= 1024;
    unitIndex++;
  }

  const formatted =
    unitIndex === 0
      ? absoluteBytes.toString()
      : absoluteBytes.toFixed(absoluteBytes < 10 ? 2 : absoluteBytes < 100 ? 1 : 0);

  return `${isNegative ? '-' : ''}${formatted} ${BYTE_UNITS[unitIndex]}`;
}

/**
 * Formats a duration in milliseconds into a human-readable string.
 *
 * Examples:
 *   0.5   -> "0.5ms"
 *   150   -> "150ms"
 *   1500  -> "1.5s"
 *   90000 -> "1.5m"
 */
export function formatDuration(ms: number): string {
  if (ms < 1000) {
    return `${Number(ms.toFixed(1))}ms`;
  }

  const seconds = ms / 1000;
  if (seconds < 60) {
    return `${Number(seconds.toFixed(1))}s`;
  }

  const minutes = seconds / 60;
  if (minutes < 60) {
    return `${Number(minutes.toFixed(1))}m`;
  }

  const hours = minutes / 60;
  return `${Number(hours.toFixed(1))}h`;
}
