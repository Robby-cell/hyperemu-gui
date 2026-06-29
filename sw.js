const cacheName = 'hyperemu-pwa-v1';
const filesToCache =[
  './',
  './index.html',
  './manifest.json',
];

// Install the service worker and cache static assets
self.addEventListener('install', e => {
  e.waitUntil(
    caches.open(cacheName).then(cache => {
      return cache.addAll(filesToCache);
    })
  );
});

// Clear old caches when a new service worker activates (e.g. when you change cacheName)
self.addEventListener('activate', e => {
  e.waitUntil(
    caches.keys().then(keyList => {
      return Promise.all(keyList.map(key => {
        if (key !== cacheName) {
          return caches.delete(key);
        }
      }));
    })
  );
});

// Intercept fetch requests and dynamically cache the Trunk outputs
self.addEventListener('fetch', e => {
  e.respondWith(
    caches.match(e.request).then(response => {
      // If found in cache, return it. Otherwise, fetch from network.
      return response || fetch(e.request).then(fetchResponse => {
        return caches.open(cacheName).then(cache => {
          // Only cache HTTP/HTTPS requests (prevents errors with browser extensions)
          if (e.request.url.startsWith('http')) {
            cache.put(e.request, fetchResponse.clone());
          }
          return fetchResponse;
        });
      });
    })
  );
});