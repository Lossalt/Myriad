import type { PhantasiNoteDoc } from '../../../types/phantasi'
import type { NoteCloudFields } from './useNoteCloudSave'
import assert from 'node:assert/strict'
import { it } from 'node:test'
import { act, createElement, useRef, useState } from 'react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { readNoteRecovery } from './noteDraft'
import { deferred, mountNoteHook } from './noteHookTestUtils'
import { useNoteCloudSave } from './useNoteCloudSave'
import { useNoteEditorOpen } from './useNoteEditorOpen'
import { useNotePublish } from './useNotePublish'

const base: NoteCloudFields = { title: 'Title', contentMd: 'a', topic: null, cover: null, publishedAt: 1 }
function doc(status: PhantasiNoteDoc['status']): PhantasiNoteDoc {
  return { id: 7, item_id: 99, title: 'Title', content_md: 'a', topic: null, image: null,
    published_at: 1, scheduled_at: status === 'scheduled' ? 60_000 : null, status, revision: 2, last_error: null, updated_at: 1 }
}

for (const [command, status] of [
  ['handleSave', 'published'], ['handleSchedule', 'scheduled'], ['handleUnschedule', 'draft'],
] as const) {
  it(`${command} recognizes its own WS receipt before HTTP and still applies HTTP metadata`, async () => {
    const raw = '```md\n[^unused]: literal\n\n\nkeep spacing\n```'
    const submitted = { ...base, contentMd: raw }
    const responseDoc = { ...doc(status), content_md: raw }
    const originalFetch = globalThis.fetch
    const response = deferred<Response>()
    let request: Record<string, unknown> | undefined
    globalThis.fetch = async (url, options) => {
      if (String(url).endsWith('/csrf-token')) return Response.json({ csrf_token: null })
      request = JSON.parse(String(options?.body))
      return response.promise
    }
    const hook = await mountNoteHook(() => {
      const [fields, setFields] = useState(submitted)
      const [docStatus, setDocStatus] = useState<PhantasiNoteDoc['status']>('draft')
      const revisionRef = useRef(1)
      const cloud = useNoteCloudSave({ userId: 1, cloudId: 7, loading: false, fields, revisionRef,
        onMerged: setFields, onSaved: () => {}, onServerDoc: (doc) => setDocStatus(doc.status), onError: () => {},
        labels: { saveFailed: 'failed', conflict: 'conflict' },
        io: { updateNoteDoc: async () => responseDoc, getNoteDoc: async () => responseDoc },
      })
      const commands = useNotePublish({ ...fields, cloudId: 7, draftKey: 'user:1:doc:7', docStatus, scheduledAt: Date.now() + 60_000,
        saving: false, setSaving: () => {}, setContentMd: (contentMd) => setFields(value => ({ ...value, contentMd })),
        setPublishedAt: (publishedAt) => setFields(value => ({ ...value, publishedAt })), revisionRef,
        runWrite: cloud.runWrite,
        onSaved: () => {}, onClose: () => {}, format: (value) => value,
        labels: { titleRequired: 'title', titleTooLong: 'long', bodyTooLong: 'long', saveFailed: 'failed',
          scheduleNeedTime: 'time', schedulePast: 'past', deleteConfirm: 'delete', deleteFailed: 'failed', discardConfirm: 'discard' },
      })
      return { commands, cloud, fields, setFields, docStatus }
    }, undefined)
    let operation!: Promise<void>
    try {
      await act(async () => { operation = hook.current.commands[command](); await Promise.resolve() })
      assert.equal(typeof request?.client_request_id, 'string')
      assert.equal(request?.revision, 1)
      if (command !== 'handleUnschedule') assert.equal(request?.content_md, raw)
      await act(async () => hook.current.setFields({ ...base, contentMd: 'ab' }))
      await act(async () => hook.current.cloud.receiveRemote(submitted, 2, request?.client_request_id as string))
      assert.equal(hook.current.fields.contentMd, 'ab')
      await act(async () => {
        response.resolve(Response.json({ success: true, id: 99, link: '/blog/posts/n99/', doc: responseDoc }))
        await operation
      })
      assert.equal(hook.current.fields.contentMd, 'ab')
      assert.equal(hook.current.docStatus, status)
    } finally {
      response.resolve(Response.json({ success: true, id: 99, link: '/blog/posts/n99/', doc: responseDoc }))
      if (operation) await act(async () => operation)
      await hook.close()
      globalThis.fetch = originalFetch
    }
  })
}

for (const composing of [false, true]) {
it(`publishing preserves continued typing before onSaved unmounts (IME ${composing}), and reopen recovers it`, async () => {
  const environment = await mountNoteHook(() => null, undefined)
  const originalFetch = globalThis.fetch
  const response = deferred<Response>()
  const published = doc('published')
  globalThis.fetch = async (url, options) => {
    if (String(url).endsWith('/csrf-token')) return Response.json({ csrf_token: null })
    return options?.method === 'POST'
      ? response.promise
      : Response.json({ success: true, doc: published })
  }
  const container = document.createElement('div')
  let editor = createRoot(container)
  let save!: () => Promise<void>
  let edit!: (fields: NoteCloudFields) => void
  let reopened: NoteCloudFields | undefined
  let cloudSession!: ReturnType<typeof useNoteCloudSave>
  let closed = false
  function Editor() {
    const [fields, setFields] = useState({ ...base, contentMd: '' })
    edit = setFields
    const revisionRef = useRef(1)
    const cloud = useNoteCloudSave({ userId: 1, cloudId: 7, loading: false, fields, revisionRef,
      onMerged: setFields, onSaved: () => {}, onServerDoc: () => {}, onError: () => {},
      labels: { saveFailed: 'failed', conflict: 'conflict' },
      io: { updateNoteDoc: async () => published, getNoteDoc: async () => published },
    })
    cloudSession = cloud
    save = useNotePublish({ ...fields, cloudId: 7, draftKey: 'user:1:doc:7', docStatus: 'draft', scheduledAt: null,
      saving: false, setSaving: () => {}, setContentMd: () => {}, setPublishedAt: () => {}, revisionRef,
      runWrite: cloud.runWrite, onSaved: () => { closed = true; flushSync(() => editor.unmount()) }, onClose: () => {}, format: value => value,
      labels: { titleRequired: 'title', titleTooLong: 'long', bodyTooLong: 'long', saveFailed: 'failed',
        scheduleNeedTime: 'time', schedulePast: 'past', deleteConfirm: 'delete', deleteFailed: 'failed', discardConfirm: 'discard' },
    }).handleSave
    return null
  }
  function Reopened() {
    useNoteEditorOpen({ userId: 1, docId: 7, cloudAck: () => {}, applyServerDoc: () => {},
      applyMergedFields: value => { reopened = value }, setCloudId: () => {}, setSaved: () => {}, setLoading: () => {},
      loadFailed: 'failed', scheduleFailed: 'failed',
    })
    return null
  }
  try {
    await act(async () => editor.render(createElement(Editor)))
    await act(async () => edit(base))
    let operation!: Promise<void>
    await act(async () => { operation = save(); await Promise.resolve() })
    if (composing) cloudSession.compositionStart()
    await act(async () => edit({ ...base, contentMd: 'ab' }))
    await act(async () => {
      response.resolve(Response.json({ success: true, id: 99, link: '/blog/posts/n99/', doc: published }))
      await Promise.resolve()
    })
    if (composing) {
      assert.equal(closed, false, 'publication must wait until composition and its receipt are settled')
      await act(async () => { cloudSession.compositionEnd(); await operation })
    } else { await act(async () => operation)
}
    assert.equal(readNoteRecovery({ userId: 1, docId: 7 })?.fields.contentMd, 'ab')
    editor = createRoot(container)
    await act(async () => editor.render(createElement(Reopened)))
    assert.equal(reopened?.contentMd, 'ab')
  } finally {
    await act(async () => editor.unmount())
    await environment.close()
    globalThis.fetch = originalFetch
  }
})
}
