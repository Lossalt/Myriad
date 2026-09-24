import assert from 'node:assert/strict'
import { afterEach, beforeEach, describe, it, mock } from 'node:test'
import { ensureSessionStoragePolyfill } from '../test/sessionStoragePolyfill'
import { clearCSRFToken } from '../utils/csrf'
import { ApiError } from './api'
import { federationApi, newPublishIdempotencyKey, PUBLISH_ATTEMPTS } from './federationApi'

type Reply = { status: number, body: unknown } | 'drop'

/** Publishing sends one Idempotency-Key per user action and reuses it on every retry. */
describe('federation publish idempotency', () => {
  let replies: Reply[]
  let sent: Array<{ url: string, headers: Record<string, string> }>
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, 'window')
  const originalLocalStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
  const token = `v1.${'a'.repeat(100)}.${'b'.repeat(43)}`
  const published = {
    success: true,
    activity_id: 'https://myriad.test/activities/1',
    content_type: 'note',
    content_id: 'note_1',
    visibility: 'public',
  }

  beforeEach(() => {
    replies = []
    sent = []
    ensureSessionStoragePolyfill()
    Object.defineProperty(globalThis, 'localStorage', { value: sessionStorage, configurable: true })
    Object.defineProperty(globalThis, 'window', {
      value: Object.assign(new EventTarget(), { location: { origin: 'https://myriad.test' } }),
      configurable: true,
    })
    clearCSRFToken()
    mock.method(globalThis, 'fetch', async (input: RequestInfo | URL, init: RequestInit = {}) => {
      const url = String(input)
      if (url.endsWith('/api/csrf-token')) return Response.json({ csrf_token: token, expires_in: 3600 })
      if (url.endsWith('/api/config/public')) return Response.json({ aiAvailability: { image: false } })
      sent.push({ url, headers: { ...(init.headers as Record<string, string>) } })
      const reply = replies.shift() ?? { status: 500, body: { error: 'Unexpected request' } }
      // The server may have committed; the client never hears back.
      if (reply === 'drop') throw new TypeError('Failed to fetch')
      return Response.json(reply.body, { status: reply.status })
    })
  })

  afterEach(() => {
    clearCSRFToken()
    mock.restoreAll()
    if (originalWindow) Object.defineProperty(globalThis, 'window', originalWindow)
    else Reflect.deleteProperty(globalThis, 'window')
    if (originalLocalStorage) Object.defineProperty(globalThis, 'localStorage', originalLocalStorage)
    else Reflect.deleteProperty(globalThis, 'localStorage')
  })

  it('retries a dropped createNote with the same key and returns the replayed result', async () => {
    replies.push('drop', { status: 200, body: published })
    const result = await federationApi.createNote({ text: 'hello' }, 'grant-1')
    assert.deepEqual(result, published)
    assert.equal(sent.length, 2)
    const [first, retry] = sent
    assert.ok(first.url.endsWith('/api/federation/notes'))
    assert.match(first.headers['Idempotency-Key'], /^publish-[0-9a-f-]{36}$/)
    assert.equal(retry.headers['Idempotency-Key'], first.headers['Idempotency-Key'])
    assert.equal(retry.headers['X-Tapp-Runtime-Grant'], 'grant-1')
  })

  it('uses a new key for each user action and the caller key when given', async () => {
    replies.push({ status: 200, body: published }, { status: 200, body: published })
    await federationApi.createNote({ text: 'one' })
    await federationApi.createNote({ text: 'two' })
    assert.notEqual(sent[0].headers['Idempotency-Key'], sent[1].headers['Idempotency-Key'])
    assert.equal(sent[0].headers['X-Tapp-Runtime-Grant'], undefined)

    const key = newPublishIdempotencyKey()
    replies.push({ status: 200, body: published })
    await federationApi.publish({ content_type: 'note', text: 'three' }, undefined, key)
    assert.ok(sent[2].url.endsWith('/api/federation/publish'))
    assert.equal(sent[2].headers['Idempotency-Key'], key)
  })

  it('does not retry an answered failure and gives up after the attempt budget', async () => {
    replies.push({ status: 409, body: { error: 'Idempotency-Key was already used for a different request' } })
    await assert.rejects(federationApi.createNote({ text: 'x' }), (error: unknown) =>
      error instanceof ApiError && error.status === 409)
    assert.equal(sent.length, 1)

    sent = []
    replies.push(...Array.from({ length: PUBLISH_ATTEMPTS }, () => 'drop' as const))
    await assert.rejects(federationApi.createNote({ text: 'y' }), (error: unknown) =>
      error instanceof ApiError && error.status === 0)
    assert.equal(sent.length, PUBLISH_ATTEMPTS)
    assert.equal(new Set(sent.map(request => request.headers['Idempotency-Key'])).size, 1)
  })
})
