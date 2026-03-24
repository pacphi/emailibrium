/// <reference lib="webworker" />

declare const self: ServiceWorkerGlobalScope;

const CACHE_NAME = 'emailibrium-v1';

const STATIC_ASSETS: string[] = ['/', '/index.html', '/manifest.json'];

// ---------------------------------------------------------------------------
// Install: pre-cache static assets
// ---------------------------------------------------------------------------
self.addEventListener('install', (event: ExtendableEvent) => {
  event.waitUntil(
    caches
      .open(CACHE_NAME)
      .then((cache) => cache.addAll(STATIC_ASSETS))
      .then(() => self.skipWaiting()),
  );
});

// ---------------------------------------------------------------------------
// Activate: purge old caches
// ---------------------------------------------------------------------------
self.addEventListener('activate', (event: ExtendableEvent) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(keys.filter((key) => key !== CACHE_NAME).map((key) => caches.delete(key))),
      )
      .then(() => self.clients.claim()),
  );
});

// ---------------------------------------------------------------------------
// Fetch: Network-first for API requests, Cache-first for static assets
// ---------------------------------------------------------------------------
self.addEventListener('fetch', (event: FetchEvent) => {
  const { request } = event;

  // Only handle GET requests
  if (request.method !== 'GET') {
    return;
  }

  if (request.url.includes('/api/')) {
    // Network-first strategy for API calls
    event.respondWith(networkFirstStrategy(request));
  } else {
    // Cache-first strategy for static assets
    event.respondWith(cacheFirstStrategy(request));
  }
});

/**
 * Network-first: try the network, fall back to cache.
 * Also updates the cache with fresh responses.
 */
async function networkFirstStrategy(request: Request): Promise<Response> {
  try {
    const response = await fetch(request);

    // Cache successful responses for offline fallback
    if (response.ok) {
      const cache = await caches.open(CACHE_NAME);
      cache.put(request, response.clone());
    }

    return response;
  } catch {
    const cached = await caches.match(request);
    if (cached) {
      return cached;
    }

    // Return a generic offline response for API requests
    return new Response(
      JSON.stringify({ error: 'offline', message: 'You are currently offline.' }),
      {
        status: 503,
        headers: { 'Content-Type': 'application/json' },
      },
    );
  }
}

/**
 * Cache-first: serve from cache if available, otherwise fetch from
 * network and update the cache.
 */
async function cacheFirstStrategy(request: Request): Promise<Response> {
  const cached = await caches.match(request);
  if (cached) {
    return cached;
  }

  try {
    const response = await fetch(request);

    if (response.ok) {
      const cache = await caches.open(CACHE_NAME);
      cache.put(request, response.clone());
    }

    return response;
  } catch {
    // Return a basic offline page for navigation requests
    if (request.mode === 'navigate') {
      const offlineHtml = await caches.match('/index.html');
      if (offlineHtml) {
        return offlineHtml;
      }
    }

    return new Response('Offline', {
      status: 503,
      headers: { 'Content-Type': 'text/plain' },
    });
  }
}

// Required to satisfy the module system when compiled
export {};
