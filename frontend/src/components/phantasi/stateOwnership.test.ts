import type { PhantasiNoteDoc } from '../../types/phantasi'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { it } from 'node:test'
import { act, createElement, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { phantasiItemState } from '../../utils/phantasiItemState'
import { requestCache } from '../../utils/requestCache'
import { loadNoteDocs, NOTE_DOCS_CACHE_KEY } from './pageData'
import { usePhantasiCategories } from './usePhantasiCategories'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', {
  paths: [require.resolve('isomorphic-dompurify')],
}))

async function withDom(run: (root: ReturnType<typeof createRoot>) => Promise<void>) {
  const dom = new JSDOM('<div id="root"></div>', { url: 'https://test.invalid' })
  const values = {
    window: dom.window,
    document: dom.window.document,
    localStorage: dom.window.localStorage,
    sessionStorage: dom.window.sessionStorage,
    IS_REACT_ACT_ENVIRONMENT: true,
  }
  const previous = new Map(Object.keys(values).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  const fetch = globalThis.fetch
  for (const [key, value] of Object.entries(values)) {
    Object.defineProperty(globalThis, key, { configurable: true, value })
  }
  const root = createRoot(dom.window.document.getElementById('root'))
  try {
    phantasiItemState.clear()
    requestCache.deleteByPrefix('phantasi:')
    await run(root)
  } finally {
    await act(async () => root.unmount())
    globalThis.fetch = fetch
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
    dom.window.close()
  }
}

it('a partial category write refreshes completed changes even when a later revision conflicts', async () => {
  await withDom(async (root) => {
    const docs = [1, 2].map(id => ({
      id, item_id: null, revision: 1, status: 'draft', title: `note ${id}`,
      content_md: '', topic: null, image: null,
    } as PhantasiNoteDoc))
    globalThis.fetch = async (input, init) => {
      const url = new URL(String(input), 'https://test.invalid')
      if (url.pathname.includes('csrf-token')) return Response.json({ csrf_token: null })
      if (init?.method === 'PUT') {
        if (url.pathname.includes('/2/')) return Response.json({ error: 'revision conflict' }, { status: 409 })
        return Response.json({ success: true, doc: { ...docs[0], topic: 'science', revision: 2 } })
      }
      return Response.json({ categories: [] })
    }
    let categories!: ReturnType<typeof usePhantasiCategories>
    let refreshed = 0
    const errors: string[] = []
    const report = (message: string) => { errors.push(message) }
    function Harness() {
      categories = usePhantasiCategories(true, docs, {
        loadFailed: 'load', createFailed: 'create', renameFailed: 'rename',
        deleteFailed: 'delete', assignFailed: 'assign', categoryFull: 'full', untitled: 'untitled',
      }, report, () => { refreshed++ })
      return null
    }
    await act(async () => root.render(createElement(Harness)))
    await act(async () => { assert.equal(await categories.assign([1, 2], 'science'), false) })
    assert.equal(refreshed, 1)
    assert.equal(errors.length, 1)
    assert.equal(categories.busy, false)
  })
})

for (const operation of ['assign', 'rename'] as const) {
  it(`a failed first category ${operation} refreshes the revision before retry`, async () => {
    await withDom(async (root) => {
      const stale = { id: 1, item_id: 3, revision: 1, status: 'published', title: 'note',
        content_md: '', topic: 'old', image: null } as PhantasiNoteDoc
      requestCache.set(NOTE_DOCS_CACHE_KEY, [stale], 60_000)
      const revisions: number[] = []
      globalThis.fetch = async (input, init) => {
        const url = new URL(String(input), 'https://test.invalid')
        if (url.pathname.includes('csrf-token')) return Response.json({ csrf_token: null })
        if (init?.method === 'PUT') {
          const revision = JSON.parse(String(init.body)).revision
          revisions.push(revision)
          return revision === 1
            ? Response.json({ error: 'revision conflict' }, { status: 409 })
            : Response.json({ success: true, doc: { ...stale, revision: 3, topic: 'new' } })
        }
        if (url.pathname.endsWith('/notes/docs')) return Response.json({ docs: [{ ...stale, revision: 2 }] })
        return Response.json({ categories: [] })
      }
      let categories!: ReturnType<typeof usePhantasiCategories>
      let pending = Promise.resolve()
      const report = () => {}
      function Harness() {
        const [docs, setDocs] = useState([stale])
        categories = usePhantasiCategories(true, docs, {
          loadFailed: 'load', createFailed: 'create', renameFailed: 'rename',
          deleteFailed: 'delete', assignFailed: 'assign', categoryFull: 'full', untitled: 'untitled',
        }, report, () => { pending = loadNoteDocs().then(setDocs) })
        return null
      }
      await act(async () => root.render(createElement(Harness)))
      const attempt = () => operation === 'assign'
        ? categories.assign([1], 'new') : categories.rename('old', 'new')
      await act(async () => { assert.equal(await attempt(), false); await pending })
      await act(async () => { assert.equal(await attempt(), true); await pending })
      assert.deepEqual(revisions, [1, 2])
    })
  })
}

it('changing a published note category sends a revision-checked metadata command, never its draft body', async () => {
  await withDom(async (root) => {
    const writes: Array<{ path: string; body: unknown }> = []
    const doc = {
      id: 8, item_id: 3, revision: 12, status: 'published', title: 'unpublished title',
      content_md: 'private unfinished draft', topic: null, image: null,
    } as PhantasiNoteDoc
    globalThis.fetch = async (input, init) => {
      const url = new URL(String(input), 'https://test.invalid')
      if (url.pathname.includes('csrf-token')) return Response.json({ csrf_token: null })
      if (init?.method === 'PUT') {
        writes.push({ path: url.pathname, body: JSON.parse(String(init.body)) })
        return Response.json({ success: true, doc: { ...doc, topic: 'science', revision: 13 } })
      }
      return Response.json({ categories: [] })
    }
    let categories!: ReturnType<typeof usePhantasiCategories>
    let refreshed = 0
    const errors: string[] = []
    const report = (message: string) => { errors.push(message) }
    const docs = [doc]
    function Harness() {
      categories = usePhantasiCategories(true, docs, {
        loadFailed: 'load', createFailed: 'create', renameFailed: 'rename',
        deleteFailed: 'delete', assignFailed: 'assign', categoryFull: 'full', untitled: 'untitled',
      }, report, () => { refreshed++ })
      return null
    }
    await act(async () => root.render(createElement(Harness)))
    await act(async () => { assert.equal(await categories.assign([8], 'science'), true) })
    assert.deepEqual(writes, [{ path: '/api/phantasi/notes/docs/8/topic', body: { topic: 'science', revision: 12 } }])
    assert.equal(refreshed, 1)
    assert.deepEqual(errors, [])
  })
})
