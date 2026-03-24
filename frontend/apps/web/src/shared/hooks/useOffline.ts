import { useEffect, useState } from 'react';

/**
 * Returns `true` when the browser reports that the user is offline.
 * Subscribes to the `online` and `offline` window events for live updates.
 */
export function useOffline(): boolean {
  const [isOffline, setIsOffline] = useState<boolean>(() => {
    if (typeof navigator === 'undefined') {
      return false;
    }
    return !navigator.onLine;
  });

  useEffect(() => {
    const handleOnline = (): void => setIsOffline(false);
    const handleOffline = (): void => setIsOffline(true);

    window.addEventListener('online', handleOnline);
    window.addEventListener('offline', handleOffline);

    return () => {
      window.removeEventListener('online', handleOnline);
      window.removeEventListener('offline', handleOffline);
    };
  }, []);

  return isOffline;
}
