import type { PhantasiNoteDoc } from '../../../types/phantasi'
import type { NoteCloudFields } from './useNoteCloudSave'
import assert from 'node:assert/strict'
import { it } from 'node:test'
import { act, useRef, useState } from 'react'
import { readNoteRecovery, writeNoteDraft, writeNoteRecovery } from './noteDraft'
import { deferred, mountNoteHook } from './noteHookTestUtils'
import { useNoteCloudSave } from './useNoteCloudSave'
import { useNoteCollab } from './useNoteCollab'
import { useNoteEditorOpen } from './useNoteEditorOpen'
import { useNotePublish } from './useNotePublish'

const base: NoteCloudFields = { title: 'Title', contentMd: 'first\nsecond', topic: 'old', cover: '/old.png', publishedAt: 1 }
function doc(revision: number, content = base.contentMd): PhantasiNoteDoc {
  return { id: 7, item_id: null, title: 'Title', content_md: content, topic: 'old', image: '/old.png',
    published_at: 1, scheduled_at: null, status: 'draft', revision, last_error: null, updated_at: 1 }
}

it('opening a document ignores ambiguous numeric recovery and merges canonical unconfirmed edits', async () => {
  const originalFetch = globalThis.fetch
  globalThis.fetch = async () => Response.json({ success: true, doc: doc(3, 'first\nREMOTE') })
  let fields: NoteCloudFields | undefined
  let saved: NoteCloudFields | undefined
  let initialized = false
  const hook = await mountNoteHook(() => {
    if (!initialized) {
      initialized = true
      writeNoteDraft(7, { ...base, contentMd: 'UNRELATED ITEM' })
      writeNoteRecovery({ userId: 1, docId: 7 }, { ...base, contentMd: 'LOCAL\nsecond' }, base, 2)
    }
    useNoteEditorOpen({ userId: 1, docId: 7, cloudAck: () => {}, applyServerDoc: () => {},
      applyMergedFields: (value) => { fields = value }, setCloudId: () => {},
      setSaved: (value) => { saved = value }, setLoading: () => {}, loadFailed: 'failed', scheduleFailed: 'failed',
    })
  }, undefined)
  try {
    assert.equal(fields?.contentMd, 'LOCAL\nREMOTE')
    assert.equal(saved?.contentMd, 'first\nREMOTE')
  } finally { await hook.close(); globalThis.fetch = originalFetch }
})

it('collaboration preserves explicit remote clears while omitted fields stay unchanged', async () => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'WebSocket')
  const sockets: FakeSocket[] = []
  class FakeSocket {
    static OPEN = 1
    readyState = 1
    onmessage: ((event: { data: string }) => void) | null = null
    constructor() { sockets.push(this) }
    send() {}
    close() {}
  }
  Object.defineProperty(globalThis, 'WebSocket', { value: FakeSocket, configurable: true })
  const hook = await mountNoteHook(() => {
    const [fields, setFields] = useState(base)
    const revisionRef = useRef(1)
    const textareaRef = useRef<HTMLTextAreaElement | null>(null)
    const cloud = useNoteCloudSave({ userId: 1, cloudId: 7, loading: false, fields, revisionRef,
      onMerged: setFields, onSaved: () => {}, onServerDoc: () => {}, onError: () => {},
      labels: { saveFailed: 'failed', conflict: 'conflict' },
    })
    useNoteCollab({ cloudId: 7, loading: false, userName: 'author', textareaRef, fields, cloud })
    return { fields }
  }, undefined)
  try {
    await act(async () => sockets[0].onmessage!({ data: JSON.stringify({ type: 'edit', peer_id: 'other', user_id: 2, title: 'New title' }) }))
    assert.equal(hook.current.fields.topic, 'old')
    assert.equal(hook.current.fields.cover, '/old.png')
    await act(async () => sockets[0].onmessage!({ data: JSON.stringify({ type: 'doc', revision: 2, peer_id: '', user_id: 2, title: base.title, content_md: base.contentMd, topic: null, image: null }) }))
    assert.equal(hook.current.fields.topic, null)
    assert.equal(hook.current.fields.cover, null)
  } finally {
    await hook.close()
    if (previous) Object.defineProperty(globalThis, 'WebSocket', previous)
    else Reflect.deleteProperty(globalThis, 'WebSocket')
  }
})

it('a persisted collaborative snapshot does not acknowledge an unsent publication time', async () => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'WebSocket')
  const sockets: FakeSocket[] = []
  class FakeSocket {
    static OPEN = 1
    readyState = 1
    onmessage: ((event: { data: string }) => void) | null = null
    constructor() { sockets.push(this) }
    send() {}
    close() {}
  }
  Object.defineProperty(globalThis, 'WebSocket', { value: FakeSocket, configurable: true })
  const writes: Array<number | null | undefined> = []
  const hook = await mountNoteHook(() => {
    const [fields, setFields] = useState(base)
    const [, setSaved] = useState(base)
    const revisionRef = useRef(1)
    const cloud = useNoteCloudSave({ userId: 1, cloudId: 7, loading: false, fields, revisionRef,
      onServerDoc: () => {}, onMerged: setFields, onSaved: setSaved, onError: () => {},
      labels: { saveFailed: 'failed', conflict: 'conflict' },
      io: { getNoteDoc: async () => doc(2), updateNoteDoc: async (_id, input) => {
        writes.push(input.published_at)
        return { ...doc(3, input.content_md), published_at: input.published_at ?? null }
      } },
    })
    const textareaRef = useRef<HTMLTextAreaElement | null>(null)
    useNoteCollab({ cloudId: 7, loading: false, textareaRef, fields, cloud })
    return { fields, setFields }
  }, undefined)
  try {
    await act(async () => hook.current.setFields({ ...base, publishedAt: 2 }))
    await act(async () => window.dispatchEvent(new window.Event('pagehide')))
    assert.equal(readNoteRecovery({ userId: 1, docId: 7 })?.fields.publishedAt, 2)
    await act(async () => sockets[0].onmessage!({ data: JSON.stringify({
      type: 'doc', revision: 2, peer_id: '', user_id: 2,
      title: base.title, content_md: 'remote body', topic: base.topic, image: base.cover,
    }) }))
    assert.equal(hook.current.fields.publishedAt, 2)
    assert.equal(readNoteRecovery({ userId: 1, docId: 7 })?.fields.publishedAt, 2)
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 850)) })
    assert.deepEqual(writes, [2])
  } finally {
    await hook.close()
    if (previous) Object.defineProperty(globalThis, 'WebSocket', previous)
    else Reflect.deleteProperty(globalThis, 'WebSocket')
  }
})

for (const command of ['handleSave', 'handleSchedule', 'handleUnschedule'] as const) {
  it(`${command} shares the document write queue with pending autosave`, async () => {
    const originalFetch = globalThis.fetch
    const pending = deferred<PhantasiNoteDoc>()
    const requests: Array<{ url: string; revision: number }> = []
    globalThis.fetch = async (url, options) => {
      if (String(url).endsWith('/csrf-token')) return Response.json({ csrf_token: null })
      requests.push({ url: String(url), revision: JSON.parse(String(options?.body)).revision })
      return Response.json({ success: true, id: 99, link: '/blog/posts/n99/', doc: doc(3, 'edited') })
    }
    const revisionRef = { current: 1 }
    const hook = await mountNoteHook((fields: NoteCloudFields) => {
      const cloud = useNoteCloudSave({ userId: 1, cloudId: 7, loading: false, fields, revisionRef,
        onServerDoc: () => {}, onMerged: () => {}, onSaved: () => {}, onError: () => {},
        labels: { saveFailed: 'failed', conflict: 'conflict' },
        io: { updateNoteDoc: async () => pending.promise, getNoteDoc: async () => doc(1) },
      })
      return useNotePublish({ ...fields, cloudId: 7, draftKey: 'user:1:doc:7', docStatus: 'draft', scheduledAt: Date.now() + 60_000,
        saving: false, setSaving: () => {}, setContentMd: () => {}, setPublishedAt: () => {}, revisionRef,
        runWrite: cloud.runWrite,
        onSaved: () => {}, onClose: () => {}, format: (value) => value,
        labels: { titleRequired: 'title', titleTooLong: 'long', bodyTooLong: 'long', saveFailed: 'failed',
          scheduleNeedTime: 'time', schedulePast: 'past', deleteConfirm: 'delete', deleteFailed: 'failed', discardConfirm: 'discard' },
      })
    }, base)
    try {
      await hook.render({ ...base, contentMd: 'edited' })
      await act(async () => { await new Promise((resolve) => setTimeout(resolve, 850)) })
      let operation!: Promise<void>
      await act(async () => { operation = hook.current[command](); await Promise.resolve() })
      assert.equal(requests.length, 0, 'explicit command must wait until autosave acknowledges its revision')
      await act(async () => { pending.resolve(doc(2, 'edited')); await operation })
      assert.equal(requests.length, 1)
      assert.equal(requests[0].revision, 2)
    } finally { pending.resolve(doc(2)); await hook.close(); globalThis.fetch = originalFetch }
  })
}

it('an explicit write response cannot undo a newer collaborative revision', async () => {
  const originalFetch = globalThis.fetch
  const pending = deferred<Response>()
  globalThis.fetch = async (url) => String(url).endsWith('/csrf-token')
    ? Response.json({ csrf_token: null }) : pending.promise
  const revisionRef = { current: 1 }
  let acknowledged = base
  const hook = await mountNoteHook(() => {
    const cloud = useNoteCloudSave({ userId: 1, cloudId: 7, loading: false, fields: base, revisionRef,
      onServerDoc: () => {}, onMerged: () => {}, onSaved: () => {}, onError: () => {},
      labels: { saveFailed: 'failed', conflict: 'conflict' },
    })
    return useNotePublish({ ...base, cloudId: 7, draftKey: 'user:1:doc:7', docStatus: 'scheduled', scheduledAt: Date.now() + 60_000,
      saving: false, setSaving: () => {}, setContentMd: () => {}, setPublishedAt: () => {}, revisionRef,
      runWrite: cloud.runWrite,
      onSaved: () => {}, onClose: () => {}, format: (value) => value,
      labels: { titleRequired: 'title', titleTooLong: 'long', bodyTooLong: 'long', saveFailed: 'failed',
        scheduleNeedTime: 'time', schedulePast: 'past', deleteConfirm: 'delete', deleteFailed: 'failed', discardConfirm: 'discard' },
    })
  }, undefined)
  try {
    let operation!: Promise<void>
    await act(async () => { operation = hook.current.handleUnschedule(); await Promise.resolve() })
    revisionRef.current = 3
    acknowledged = { ...base, contentMd: 'newer remote text' }
    await act(async () => { pending.resolve(Response.json({ success: true, doc: doc(2) })); await operation })
    assert.equal(revisionRef.current, 3)
    assert.equal(acknowledged.contentMd, 'newer remote text')
  } finally { pending.resolve(Response.json({ success: true, doc: doc(2) })); await hook.close(); globalThis.fetch = originalFetch }
})

it('opening a new document leaves the unowned legacy new draft untouched', async () => {
  const originalFetch = globalThis.fetch
  globalThis.fetch = async (url) => String(url).endsWith('/csrf-token')
    ? Response.json({ csrf_token: null }) : Response.json({ success: true, doc: doc(1, '') })
  let initialized = false
  let content: string | undefined
  const hook = await mountNoteHook(() => {
    if (!initialized) {
      initialized = true
      writeNoteDraft('new', { ...base, contentMd: 'another user private draft' })
    }
    useNoteEditorOpen({ userId: 20, cloudAck: () => {}, applyServerDoc: () => {},
      applyMergedFields: (fields) => { content = fields.contentMd }, setCloudId: () => {},
      setSaved: () => {}, setLoading: () => {}, loadFailed: 'failed', scheduleFailed: 'failed',
    })
  }, undefined)
  try {
    assert.equal(content, '')
    assert.match(localStorage.getItem('phantasi:note-draft:new') ?? '', /another user private draft/)
  } finally { await hook.close(); globalThis.fetch = originalFetch }
})

it('opening the same document under another user never restores the first user recovery', async () => {
  const originalFetch = globalThis.fetch
  globalThis.fetch = async () => Response.json({ success: true, doc: doc(2) })
  let initialized = false
  let content: string | undefined
  const hook = await mountNoteHook((userId: number) => {
    if (!initialized) {
      initialized = true
      writeNoteRecovery({ userId: 10, docId: 7 }, { ...base, contentMd: 'unfinished by first user' }, base, 1)
    }
    useNoteEditorOpen({ userId, docId: 7, cloudAck: () => {}, applyServerDoc: () => {},
      applyMergedFields: (fields) => { content = fields.contentMd }, setCloudId: () => {},
      setSaved: () => {}, setLoading: () => {}, loadFailed: 'failed', scheduleFailed: 'failed',
    })
  }, 10)
  try {
    assert.equal(content, 'unfinished by first user')
    await hook.render(20)
    assert.equal(content, 'first\nsecond')
    await hook.render(10)
    assert.equal(content, 'unfinished by first user')
  } finally { await hook.close(); globalThis.fetch = originalFetch }
})

it('does not reattribute old editor fields while an identity change is closing that editor', async () => {
  const hook = await mountNoteHook((input: { userId: number; fields: NoteCloudFields }) => useNoteCloudSave({
    userId: input.userId, cloudId: 7, loading: false, fields: input.fields, revisionRef: { current: 1 },
    onServerDoc: () => {}, onMerged: () => {}, onSaved: () => {}, onError: () => {},
    labels: { saveFailed: 'failed', conflict: 'conflict' },
  }), { userId: 10, fields: base })
  try {
    const fields = { ...base, contentMd: 'unfinished by first user' }
    await hook.render({ userId: 10, fields })
    await hook.render({ userId: 20, fields })
    assert.equal(readNoteRecovery({ userId: 20, docId: 7 }), null)
    assert.equal(readNoteRecovery({ userId: 10, docId: 7 })?.fields.contentMd, 'unfinished by first user')
  } finally { await hook.close() }
})

it('a real null server publication time does not replace the recovered unconfirmed date on reopen', async () => {
  const originalFetch = globalThis.fetch
  globalThis.fetch = async () => Response.json({ success: true, doc: { ...doc(2), published_at: null } })
  let initialized = false
  let recovered: number | null | undefined
  let acknowledged: number | null | undefined
  const hook = await mountNoteHook(() => {
    if (!initialized) {
      initialized = true
      writeNoteRecovery({ userId: 1, docId: 7 }, { ...base, publishedAt: 500 }, { ...base, publishedAt: null }, 1)
    }
    useNoteEditorOpen({ userId: 1, docId: 7, cloudAck: (fields) => { acknowledged = fields.publishedAt }, applyServerDoc: () => {},
      applyMergedFields: (fields) => { recovered = fields.publishedAt }, setCloudId: () => {},
      setSaved: () => {}, setLoading: () => {}, loadFailed: 'failed', scheduleFailed: 'failed',
    })
  }, undefined)
  try {
    assert.equal(recovered, 500)
    assert.equal(acknowledged, null)
  } finally { await hook.close(); globalThis.fetch = originalFetch }
})

it('opening an ambiguous recovery defers the latest server document until the saved request is identified', async () => {
  const originalFetch = globalThis.fetch
  globalThis.fetch = async () => Response.json({ success: true, doc: doc(3, 'a\nremote') })
  let initialized = false
  const requests: number[] = []
  const hook = await mountNoteHook(() => {
    if (!initialized) {
      initialized = true
      writeNoteRecovery({ userId: 1, docId: 7 }, { ...base, contentMd: 'ab' }, { ...base, contentMd: '' }, 1,
        Date.now(), { requestId: 'closed-before-response', fields: { ...base, contentMd: 'a' } })
    }
    const [fields, setFields] = useState(base)
    const [loading, setLoading] = useState(true)
    const revisionRef = useRef(1)
    const cloud = useNoteCloudSave({ userId: 1, cloudId: 7, loading, fields, revisionRef,
      onMerged: setFields, onSaved: () => {}, onServerDoc: value => { revisionRef.current = value.revision }, onError: () => {},
      labels: { saveFailed: 'failed', conflict: 'conflict', uncertain: 'unconfirmed' },
      io: { getNoteDoc: async () => doc(3, 'a\nremote'), updateNoteDoc: async (_id, input) => {
        requests.push(input.revision!); return doc(4, input.content_md)
      } },
    })
    useNoteEditorOpen({ userId: 1, docId: 7, cloudAck: cloud.ack, restorePending: cloud.restorePending,
      applyServerDoc: value => { revisionRef.current = value.revision }, applyMergedFields: setFields,
      setCloudId: () => {}, setSaved: () => {}, setLoading, loadFailed: 'failed', scheduleFailed: 'failed',
    })
    return { fields, cloud, revisionRef }
  }, undefined)
  try {
    assert.equal(hook.current.fields.contentMd, 'ab')
    assert.equal(hook.current.revisionRef.current, 1)
    assert.equal(readNoteRecovery({ userId: 1, docId: 7 })?.base.contentMd, '')
    await assert.rejects(hook.current.cloud.runWrite(async () => {}), /unconfirmed/)
    await act(async () => hook.current.cloud.receiveRemote({ ...base, contentMd: 'a' }, 2, 'closed-before-response'))
    assert.equal(hook.current.fields.contentMd, 'ab\nremote')
    assert.deepEqual(requests, [3])
  } finally { await hook.close(); globalThis.fetch = originalFetch }
})
