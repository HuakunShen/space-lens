/// <reference types="@sveltejs/kit" />
import { build, files, version } from '$service-worker'

const CACHE = `spacelens-shell-${version}`
// `build` = hashed client assets, `files` = static files; the SPA fallback
// document is written by the adapter and named explicitly here.
const ASSETS = [...build, ...files, '/200.html']

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(ASSETS))
      .then(() => self.skipWaiting()),
  )
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  )
})

self.addEventListener('fetch', (event) => {
  const request = event.request
  if (request.method !== 'GET') return
  const url = new URL(request.url)
  if (url.origin !== self.location.origin) return
  // the API is never intercepted: a cached answer to a live question is a lie
  if (url.pathname.startsWith('/api/')) return

  if (request.mode === 'navigate') {
    // network-first so a reachable server always wins; offline reloads fall
    // back to the shell, which then reports "not connected" itself
    event.respondWith(
      fetch(request)
        .then((response) => {
          const copy = response.clone()
          void caches.open(CACHE).then((cache) => cache.put('/200.html', copy))
          return response
        })
        .catch(() => caches.match('/200.html').then((hit) => hit ?? Response.error())),
    )
    return
  }

  event.respondWith(caches.match(request).then((hit) => hit ?? fetch(request)))
})
