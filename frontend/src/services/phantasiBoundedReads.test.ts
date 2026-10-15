import assert from 'node:assert/strict'
import { it } from 'node:test'
import { phantasiSubject } from '../utils/phantasiSubject'
import { getCommentReplies, getComments } from './phantasiApi'

it('collects comment and reply pages while preserving display order and cancellation', async () => {
  const original = globalThis.fetch
  const urls: string[] = []
  globalThis.fetch = async input => {
    const url = String(input)
    urls.push(url)
    const last = url.includes('after_id=2')
    const rows = last ? [{ id: 3, start_offset: 0, created_at: 1000 }] : [{ id: 1, start_offset: null, created_at: 3000 }, { id: 2, start_offset: 10, created_at: 2000 }]
    return Response.json({ success: true, comments: rows, replies: rows, can_write: true, has_comments: true, next_cursor: last ? null : 2 })
  }
  try {
    const comments = await getComments(1)
    assert.deepEqual(comments.comments.map(c => c.id), [3, 2, 1])
    assert.equal(comments.can_write, true)
    const replies = await getCommentReplies(1)
    assert.deepEqual(replies.replies.map(c => c.id), [3, 2, 1])
    assert.equal(urls.length, 4)
    const controller = new AbortController()
    controller.abort()
    await assert.rejects(getComments(1, undefined, { signal: controller.signal }), { name: 'AbortError' })
    assert.equal(urls.length, 4)
  } finally { globalThis.fetch = original }
})

it('stops on repeated pagination cursors and on cancellation between pages', async () => {
  const original = globalThis.fetch
  let calls = 0
  globalThis.fetch = async () => {
    calls++
    return Response.json({ success: true, comments: [], next_cursor: 2 })
  }
  try {
    await assert.rejects(getComments(1), /Invalid comment pagination cursor/)
    assert.equal(calls, 2)
    const controller = new AbortController()
    globalThis.fetch = async () => {
      controller.abort()
      return Response.json({ success: true, comments: [], next_cursor: 3 })
    }
    await assert.rejects(getComments(1, undefined, { signal: controller.signal }), { name: 'AbortError' })
  } finally { globalThis.fetch = original }
})

it('pins the subject across comment pages and never requests the next page after a switch', async () => {
  const original = globalThis.fetch
  const before = phantasiSubject.getSnapshot()
  let calls = 0
  globalThis.fetch = async () => {
    calls++
    const response = Response.json({})
    response.json = async () => ({
      success: true, comments: [{ id: 1 }],
      get next_cursor() {
        phantasiSubject.change('user:other')
        return calls === 1 ? 1 : null
      },
    })
    return response
  }
  try {
    await assert.rejects(getComments(1), { name: 'AbortError' })
    assert.equal(calls, 1)
  } finally {
    globalThis.fetch = original
    phantasiSubject.change(before.key, before.active)
  }
})
