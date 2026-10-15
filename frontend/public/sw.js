// 跨域一律不拦截：SW 内 fetch() 是 CORS，对端无 ACAO 会失败并被 FetchEvent 打成 uncaught。
// 图片 / CSS / JS / 字体分支只处理 GET；非 GET 放行，避免 Cache API put 报错。
const CACHE_VERSION = 'myriad-v2.8'
const STATIC_CACHE = `${CACHE_VERSION}-static`
const DYNAMIC_CACHE = `${CACHE_VERSION}-dynamic`
const IMAGE_CACHE = `${CACHE_VERSION}-images`

const STATIC_ASSETS = ['/', '/logo.webp', '/wallpapers/default.webp']

const MAX_STATIC_CACHE_SIZE = 180
const MAX_DYNAMIC_CACHE_SIZE = 50
// Must exceed the app's own icon set (~64 files) or the LRU eviction thrashes:
// icons get evicted then re-downloaded on the next screen. Headroom left for
// dynamic images (avatars, thumbnails) sharing this cache.
const MAX_IMAGE_CACHE_SIZE = 200
const CACHE_MAX_AGE = {
  static: 30 * 24 * 60 * 60 * 1000,
  images: 7 * 24 * 60 * 60 * 1000,
}

globalThis.addEventListener('install', (event) => {
  console.log('[SW] Installing Service Worker...')

  event.waitUntil(
    Promise.all(STATIC_ASSETS.map(async (asset) => {
      const response = await fetch(asset)
      if (!response.ok) throw new Error(`Precache failed: ${asset}`)
      await cacheWithTimestamp(STATIC_CACHE, asset, response)
    }))
      .catch((err) => {
        console.error('[SW] Precache failed:', err)
      }),
  )

  globalThis.skipWaiting()
})

globalThis.addEventListener('activate', (event) => {
  console.log('[SW] Activating Service Worker...')

  event.waitUntil(
    caches.keys().then(async (keys) => {
      await Promise.all(
        keys
          .filter(
            (key) =>
              key.startsWith('myriad-') &&
              key !== STATIC_CACHE &&
              key !== DYNAMIC_CACHE &&
              key !== IMAGE_CACHE,
          )
          .map((key) => {
            console.log('[SW] Removing old cache:', key)
            return caches.delete(key)
          }),
      )
      await Promise.all([
        limitCacheSize(STATIC_CACHE, MAX_STATIC_CACHE_SIZE, CACHE_MAX_AGE.static),
        limitCacheSize(IMAGE_CACHE, MAX_IMAGE_CACHE_SIZE, CACHE_MAX_AGE.images),
        limitCacheSize(DYNAMIC_CACHE, MAX_DYNAMIC_CACHE_SIZE, CACHE_MAX_AGE.images),
      ])
    }),
  )

  return globalThis.clients.claim()
})

async function limitCacheSize(cacheName, maxSize, maxAge) {
  const cache = await caches.open(cacheName)
  const keys = await cache.keys()
  const retained = []
  for (const key of keys) {
    const response = await cache.match(key)
    if (response && maxAge && isCacheExpired(response, maxAge)) await cache.delete(key)
    else retained.push(key)
  }
  await Promise.all(retained.slice(0, Math.max(0, retained.length - maxSize)).map((key) => cache.delete(key)))
}

// Serialize writes and eviction per cache so parallel requests cannot exceed the cap.
const cacheWrites = new Map()
let cacheGeneration = 0
let cacheClearing = false
function scheduleCacheWrite(event, cacheName, request, response, maxSize, maxAge) {
  if (cacheClearing) {
    void response.body?.cancel().catch(() => {})
    return
  }
  const generation = cacheGeneration
  const writing = (cacheWrites.get(cacheName) ?? Promise.resolve())
    .then(async () => {
      if (generation !== cacheGeneration) return
      await cacheWithTimestamp(cacheName, request, response)
      await limitCacheSize(cacheName, maxSize, maxAge)
    })
    .catch((error) => console.warn('[SW] Cache write failed:', error))
  cacheWrites.set(cacheName, writing)
  const cleanup = writing.finally(() => {
    if (cacheWrites.get(cacheName) === writing) cacheWrites.delete(cacheName)
  })
  event.waitUntil(cleanup)
}

function isCacheExpired(response, maxAge) {
  const cachedDate = response.headers.get('sw-cached-date')
  if (!cachedDate) return true

  const cacheTime = new Date(cachedDate).getTime()
  const now = Date.now()
  return !Number.isFinite(cacheTime) || now - cacheTime > maxAge
}

async function cacheWithTimestamp(cacheName, request, response) {
  // Cache API 只接受 GET；HEAD 会抛 unsupported method
  if (request.method && request.method !== 'GET') {
    return
  }
  if (response.status === 206) {
    console.log('[SW] Skipping cache for 206 response:', request.url)
    return
  }

  const cache = await caches.open(cacheName)
  const headers = new Headers(response.headers)
  headers.set('sw-cached-date', new Date().toISOString())

  const cachedResponse = new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  })

  await cache.put(request, cachedResponse)
}

globalThis.addEventListener('fetch', (event) => {
  const { request } = event
  const url = new URL(request.url)

  if (!url.protocol.startsWith('http')) {
    return
  }

  // 只缓存同站。拦跨域再 fetch 会变 CORS，无 ACAO 即 uncaught FetchEvent。
  if (url.origin !== globalThis.location.origin) {
    return
  }

  if (url.pathname.startsWith('/api/')) {
    // /api/ 一律直出网络，不走 SW 缓存。
    return
  }

  if (url.pathname === '/blog' || url.pathname.startsWith('/blog/')) {
    // 博客是后端生成的静态站，每次重建或换主题都会改样式表与图片，缓存只听响应头。
    return
  }

  // 图片仅 GET：Cache API 不能 put HEAD；图床探活 HEAD 必须直通。
  if (
    request.destination === 'image' ||
    /\.(jpg|jpeg|png|gif|webp|svg|avif|ico)$/i.test(url.pathname)
  ) {
    if (request.method !== 'GET') {
      return
    }

    const cacheMaxAge = CACHE_MAX_AGE.images

    event.respondWith(
      caches.match(request).then(async (cachedResponse) => {
        if (cachedResponse && !isCacheExpired(cachedResponse, cacheMaxAge)) {
          return cachedResponse
        }

        try {
          // 保持原始 request（含 mode / credentials），勿强制 cors
          const response = await fetch(request)

          if (response.ok && response.status !== 206) {
            const responseClone = response.clone()
            scheduleCacheWrite(event, IMAGE_CACHE, request, responseClone, MAX_IMAGE_CACHE_SIZE, cacheMaxAge)
          }
          return response
        } catch (error) {
          if (cachedResponse) {
            console.log(
              '[SW] Using cached image after network error:',
              request.url,
            )
            return cachedResponse
          }
          console.warn('[SW] Image fetch failed:', request.url, error)
          // 勿返回空 200 blob（会掩盖失败并搞坏 background-image）
          return Response.error()
        }
      }),
    )
    return
  }

  // CSS/JS/字体仅 GET
  if (/\.(css|js|woff2?)$/i.test(url.pathname)) {
    if (request.method !== 'GET') {
      return
    }

    event.respondWith(
      caches.match(request).then(async (cachedResponse) => {
        if (
          cachedResponse &&
          !isCacheExpired(cachedResponse, CACHE_MAX_AGE.static)
        ) {
          return cachedResponse
        }

        try {
          const response = await fetch(request)
          if (response.ok) {
            const responseClone = response.clone()
            scheduleCacheWrite(event, STATIC_CACHE, request, responseClone, MAX_STATIC_CACHE_SIZE, CACHE_MAX_AGE.static)
          }
          return response
        } catch (error) {
          if (cachedResponse) {
            return cachedResponse
          }
          throw error
        }
      }),
    )
    return
  }

  event.respondWith(
    fetch(request)
      .then((response) => {
        if (
          request.method === 'GET' &&
          response.ok &&
          response.status !== 206
        ) {
          const responseClone = response.clone()
          scheduleCacheWrite(event, DYNAMIC_CACHE, request, responseClone, MAX_DYNAMIC_CACHE_SIZE, CACHE_MAX_AGE.images)
        }
        return response
      })
      .catch(() => {
        return caches.match(request).then((cachedResponse) => {
          if (cachedResponse) {
            return cachedResponse
          }
          if (request.mode === 'navigate') {
            return caches.match('/')
          }
          return Response.error()
        })
      }),
  )
})

globalThis.addEventListener('message', (event) => {
  if (event.data && event.data.type === 'SKIP_WAITING') {
    globalThis.skipWaiting()
  }

  if (event.data && event.data.type === 'CLEAR_CACHE') {
    const generation = ++cacheGeneration
    cacheClearing = true
    event.waitUntil(
      Promise.all([...cacheWrites.values()])
        .then(() => caches.keys())
        .then((keys) => Promise.all(keys.map((key) => caches.delete(key))))
        .then(() => caches.keys())
        .then((remaining) =>
          Promise.all(remaining.map((key) => caches.delete(key))),
        )
        .then(() => {
          try {
            event.ports?.[0]?.postMessage({ success: true })
          } catch {
            // port 可能已关闭
          }
        })
        .catch(() => {
          try {
            event.ports?.[0]?.postMessage({ success: false })
          } catch {
            // ignore
          }
        })
        .finally(() => {
          if (generation === cacheGeneration) cacheClearing = false
        }),
    )
  }
})
