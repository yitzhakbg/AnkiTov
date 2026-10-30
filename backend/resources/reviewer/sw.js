/* AnkiTov reviewer service worker — v1: network-first with a minimal offline
   shell cache. Review answers REQUIRE the network (server-side ledger), so the
   SW never caches API calls — only the static shell. */
var CACHE = "ankitov-reviewer-v1";
var SHELL = ["/reviewer", "/reviewer/style.css", "/reviewer/app.js", "/reviewer/icon.svg"];

self.addEventListener("install", function (e) {
  e.waitUntil(caches.open(CACHE).then(function (c) { return c.addAll(SHELL); }));
});

self.addEventListener("fetch", function (e) {
  var url = new URL(e.request.url);
  if (url.pathname.startsWith("/api/")) return; // never cache the API
  if (e.request.method !== "GET") return;
  e.respondWith(
    fetch(e.request).then(function (resp) {
      var copy = resp.clone();
      caches.open(CACHE).then(function (c) { c.put(e.request, copy); });
      return resp;
    }).catch(function () {
      return caches.match(e.request).then(function (hit) { return hit || caches.match("/reviewer"); });
    })
  );
});
