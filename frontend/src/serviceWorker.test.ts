import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import vm from 'node:vm'

const ORIGIN = 'https://site.example'

type FetchHandler = (event: { request: Request; respondWith: (r: unknown) => void; waitUntil: () => void }) => void

function loadFetchHandler(): FetchHandler {
  const handlers = new Map<string, FetchHandler>()
  const scope = {
    location: { origin: ORIGIN },
    addEventListener: (type: string, fn: FetchHandler) => handlers.set(type, fn),
    skipWaiting: () => {},
    clients: { claim: () => Promise.resolve() },
    caches: { match: () => Promise.resolve(undefined), open: () => Promise.reject(new Error('unused')) },
    fetch: () => Promise.reject(new Error('unused')),
    console: { log: () => {}, warn: () => {}, error: () => {} },
    URL,
    Headers,
    Response,
    Date,
    Map,
    Promise,
  }
  const context = vm.createContext({ ...scope, globalThis: scope })
  Object.assign(context.globalThis, scope)
  vm.runInContext(readFileSync(new URL('../public/sw.js', import.meta.url), 'utf8'), context)
  const handler = handlers.get('fetch')
  assert.ok(handler)
  return handler
}

function intercepts(handler: FetchHandler, path: string): boolean {
  let responded = false
  handler({
    request: new Request(`${ORIGIN}${path}`),
    respondWith: (response) => {
      responded = true
      void Promise.resolve(response).catch(() => {})
    },
    waitUntil: () => {},
  })
  return responded
}

describe('service worker', () => {
  it('leaves the backend blog to HTTP caching', () => {
    const handler = loadFetchHandler()
    for (const path of ['/blog', '/blog/', '/blog/css/style.css', '/blog/_myriad/notes.css', '/blog/posts/a/cover.webp']) {
      assert.equal(intercepts(handler, path), false, path)
    }
  })

  it('still caches the SPA shell assets', () => {
    const handler = loadFetchHandler()
    assert.equal(intercepts(handler, '/assets/index-abc.css'), true)
    assert.equal(intercepts(handler, '/logo.webp'), true)
    assert.equal(intercepts(handler, '/blogroll'), true)
  })
})
