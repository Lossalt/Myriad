import assert from 'node:assert/strict'
import test from 'node:test'
import { fetchWithAiConfiguration } from './aiConfiguration.ts'

const url = '/api/agent/process/stream'
const options = { method: 'POST', body: JSON.stringify({ context: { mode: 'work' } }) }

test('missing AI configuration prevents generation and returns a stable error', async () => {
  const calls: string[] = []
  const fetcher: typeof fetch = async (input) => {
    calls.push(String(input))
    return Response.json({ aiAvailability: { standard: false } })
  }
  const response = await fetchWithAiConfiguration(url, options, fetcher)
  assert.equal(response.status, 409)
  assert.equal((await response.json()).code, 'ai_not_configured')
  assert.equal(calls.length, 1)
  assert.match(calls[0], /\/api\/config\/public$/)
})

test('saved configuration is checked again on the next operation', async () => {
  let configured = false
  let generations = 0
  const fetcher: typeof fetch = async (input) => {
    if (String(input).endsWith('/config/public')) {
      return Response.json({ aiAvailability: { standard: configured } })
    }
    generations++
    return Response.json({ ok: true })
  }
  assert.equal((await fetchWithAiConfiguration(url, options, fetcher)).status, 409)
  configured = true
  assert.equal((await fetchWithAiConfiguration(url, options, fetcher)).status, 200)
  assert.equal(generations, 1)
})

test('chat uses its own availability, and persona uses its own Pro requirement', async () => {
  const fetcher: typeof fetch = async (input) => String(input).endsWith('/config/public')
    ? Response.json({ aiAvailability: { standard: false, chat: true, persona: false } })
    : Response.json({ ok: true })
  assert.equal((await fetchWithAiConfiguration(url, {
    ...options, body: JSON.stringify({ context: { mode: 'chat' } }),
  }, fetcher)).status, 200)
  assert.equal((await fetchWithAiConfiguration('/api/agent/persona/draft', options, fetcher)).status, 409)
})

test('reading saved data and saving settings do not require AI', async () => {
  for (const [path, method] of [
    ['/api/agent/sessions', 'GET'],
    ['/api/agent/persona', 'PUT'],
    ['/api/phantasiai/items/1/annotations', 'GET'],
    ['/api/agent/session/cancel-chat', 'POST'],
  ]) {
    const calls: string[] = []
    const response = await fetchWithAiConfiguration(path, { method }, async (input) => {
      calls.push(String(input))
      return Response.json({ ok: true })
    })
    assert.equal(response.status, 200)
    assert.deepEqual(calls, [path])
  }
})

test('configuration lookup failures are not reported as missing AI', async () => {
  const response = await fetchWithAiConfiguration(url, options, async () => new Response('bad gateway', { status: 502 }))
  assert.equal(response.status, 502)
  assert.equal(await response.text(), 'bad gateway')
})

test('aborting the configuration check prevents generation', async () => {
  const controller = new AbortController()
  let calls = 0
  await assert.rejects(fetchWithAiConfiguration(url, { ...options, signal: controller.signal }, async () => {
    calls++
    controller.abort()
    return Response.json({ aiAvailability: { standard: true } })
  }), { name: 'AbortError' })
  assert.equal(calls, 1)
})

test('generation entry points check the matching capability', async () => {
  for (const [path, capability] of [
    ['/api/tapp-playground/generate-stream', 'pro'],
    ['/api/agent/persona/name', 'personaName'],
    ['/api/merope/rig/portrait', 'image'],
    ['/api/merope/rig/avatar', 'image'],
    ['/api/reports/platform', 'standard'],
    ['/api/phantasiai/items/7/annotations/regenerate', 'standard'],
    ['/api/phantasiai/items/7/podcast/regenerate', 'standard'],
    ['/api/phantasiai/sources/7/style-tags', 'standard'],
  ]) {
    let calls = 0
    const response = await fetchWithAiConfiguration(path, { method: 'POST' }, async () => {
      calls++
      return Response.json({ aiAvailability: { [capability]: false } })
    })
    assert.equal(response.status, 409, path)
    assert.equal(calls, 1, path)
  }
})

test('an older backend still validates requests and rule-based fallbacks remain usable', async () => {
  let calls = 0
  await fetchWithAiConfiguration(url, options, async () => {
    calls++
    return Response.json({})
  })
  assert.equal(calls, 2)
  for (const path of ['/api/prompt/generate', '/api/seo/generate-copy', '/api/ai/recommend-icon']) {
    const requested: string[] = []
    await fetchWithAiConfiguration(path, { method: 'POST' }, async (input) => {
      requested.push(String(input))
      return Response.json({})
    })
    assert.deepEqual(requested, [path])
  }
})

test('naming accepts strict Lite without Pro, and rejects Pro without strict Lite', async () => {
  const nameUrl = '/api/agent/persona/name'
  for (const [personaName, persona, status] of [[true, false, 200], [false, true, 409]] as const) {
    const result = await fetchWithAiConfiguration(nameUrl, options, async (input) =>
      String(input).endsWith('/config/public')
        ? Response.json({ aiAvailability: { personaName, persona } })
        : Response.json({ name: 'Merope' }))
    assert.equal(result.status, status)
  }
})

test('note formatting requires the configured standard model before sending the draft', async () => {
  const response = await fetchWithAiConfiguration('/api/phantasiai/notes/edit', { method: 'POST', body: '{"content_md":"private draft"}' }, async (input) => {
    if (String(input).endsWith('/config/public')) return Response.json({ aiAvailability: { standard: false } })
    return Response.json({ content_md: 'should not reach provider' })
  })
  assert.equal(response.status, 409)
})
