/**
 * Registers the service worker and sets up update handling.
 *
 * Call this once during application bootstrap (e.g. in main.tsx after
 * the React root is mounted).
 */
export async function registerServiceWorker(): Promise<void> {
  if (!('serviceWorker' in navigator)) {
    return;
  }

  try {
    const registration = await navigator.serviceWorker.register('/sw.js', {
      scope: '/',
    });

    // Listen for updates and notify the user
    registration.addEventListener('updatefound', () => {
      const newWorker = registration.installing;
      if (!newWorker) {
        return;
      }

      newWorker.addEventListener('statechange', () => {
        if (newWorker.state === 'installed' && navigator.serviceWorker.controller) {
          // A new version is available. The user can reload to activate it.
          dispatchUpdateEvent();
        }
      });
    });

    // Handle controller change (after skipWaiting in the new SW)
    let refreshing = false;
    navigator.serviceWorker.addEventListener('controllerchange', () => {
      if (!refreshing) {
        refreshing = true;
        window.location.reload();
      }
    });
  } catch (error) {
    console.error('[SW] Registration failed:', error);
  }
}

/**
 * Dispatches a custom event that the application can listen for to show
 * an "update available" prompt.
 */
function dispatchUpdateEvent(): void {
  window.dispatchEvent(new CustomEvent('sw-update-available'));
}

/**
 * Sends a message to the active service worker to skip waiting and
 * activate the new version immediately.
 */
export function applyServiceWorkerUpdate(): void {
  navigator.serviceWorker.controller?.postMessage({ type: 'SKIP_WAITING' });
}
