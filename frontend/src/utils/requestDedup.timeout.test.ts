import assert from 'node:assert/strict'
import test, { mock } from 'node:test'
import { clearDedupCache, getLatestReportDeduped, getPublicConfigDeduped, getUIConfigDeduped } from './requestDedup'

test('shared config and report reads abort stalled bodies and can retry afterward', async () => {
  const oldFetch = globalThis.fetch
  mock.timers.enable({ apis: ['setTimeout'] })
  try {
    for (const read of [getUIConfigDeduped, getPublicConfigDeduped, getLatestReportDeduped]) {
      clearDedupCache()
      globalThis.fetch = async (_url, options) => new Response(new ReadableStream({
        start(controller) {
          options?.signal?.addEventListener('abort', () => controller.error(options.signal!.reason), { once: true })
        },
      }), { headers: { 'content-type': 'application/json' } })
      const pending = read().then(() => 'unexpected', (error: { code?: string }) => error.code)
      // Reads go through apiService: its request timeout also covers the body.
      await Promise.resolve()
      mock.timers.tick(30_000)
      assert.equal(await pending, 'TIMEOUT')
      globalThis.fetch = async () => Response.json({ success: true, platform_reports: [{}] })
      assert.equal((await read()).success, true)
    }
  } finally {
    mock.timers.reset()
    globalThis.fetch = oldFetch
    clearDedupCache()
  }
})

test('a response cached for one identity is never served to the next', async () => {
  const { authSubject } = await import('./authSubject')
  const oldFetch = globalThis.fetch
  try {
    clearDedupCache()
    globalThis.fetch = async () => Response.json({ success: true, who: 'owner' })
    assert.equal((await getPublicConfigDeduped()).who, 'owner')
    globalThis.fetch = async () => Response.json({ success: true, who: 'guest' })
    // Still inside the TTL: only the identity change may drop the entry.
    authSubject.change(`test-subject:${Date.now()}`)
    assert.equal((await getPublicConfigDeduped()).who, 'guest')
  } finally {
    globalThis.fetch = oldFetch
    clearDedupCache()
  }
})
