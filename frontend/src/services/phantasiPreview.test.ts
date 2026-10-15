import assert from 'node:assert/strict'
import { it } from 'node:test'
import { getItem, getItemPreviews } from './phantasiApi'

it('requests body-free lists and loads the full body for the reader', async () => {
  const originalFetch = globalThis.fetch
  const urls: URL[] = []
  globalThis.fetch = async input => {
    const url = new URL(String(input), 'https://test.invalid')
    urls.push(url)
    if (url.pathname.endsWith('/items/91234')) return Response.json({ item: { id: 91234, content: '<p>detail</p>' } })
    const item = { id: 91234, title: 'preview' }
    return Response.json({ items: [item], total: 1, page: 1, per_page: 20 })
  }
  try {
    const previews = await getItemPreviews({ source_id: 8, page: 2 })
    assert.equal(urls[0].searchParams.has('projection'), false)
    assert.equal(urls[0].searchParams.get('source_id'), '8')
    assert.equal(urls[0].searchParams.get('page'), '2')
    assert.equal(Object.hasOwn(previews.items[0], 'content'), false)
    assert.equal((await getItem(91234)).content, '<p>detail</p>')
  } finally {
    globalThis.fetch = originalFetch
  }
})
