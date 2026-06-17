const CACHE = 'aegis-demo-v1';
const PRECACHE = [
  './',
  './index.html',
  '../../rust/pkg/aegis_browser.js',
  '../../rust/pkg/aegis_browser_bg.wasm',
];

self.addEventListener('install', (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(PRECACHE)));
  self.skipWaiting();
});

self.addEventListener('activate', (e) => {
  e.waitUntil(clients.claim());
});

self.addEventListener('fetch', (e) => {
  e.respondWith(caches.match(e.request).then((cached) => cached || fetch(e.request)));
});
