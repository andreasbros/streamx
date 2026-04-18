// Minimal service worker for background playback keepalive.
// No caching - StreamX streams are dynamic and auth-gated.

self.addEventListener("install", () => {
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener("fetch", () => {
  // Pass-through: let the browser handle all fetches normally
});
