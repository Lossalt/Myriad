import assert from 'node:assert/strict'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))
const root = join(dir, '../../../..')
const read = (...parts: string[]) => readFileSync(join(dir, ...parts), 'utf8')
const readRoot = (...parts: string[]) => readFileSync(join(root, ...parts), 'utf8')

function fnBody(src: string, name: string): string {
  const start = src.indexOf(`export async function ${name}`)
  assert.notEqual(start, -1, name)
  const rest = src.slice(start)
  const next = rest.slice(1).search(/\nexport (async )?function /)
  return next === -1 ? rest : rest.slice(0, next + 1)
}

describe('手帐审计契约', () => {
  it('批量刷新每源跳过缓存、结束只清一次', () => {
    const api = read('../../services/phantasiApi.ts')
    const body = fnBody(api, 'refreshSources')
    assert.match(body, /skipCache: true/)
    assert.equal(body.split('invalidateSourcesCache()').length - 1, 1)
    assert.equal(body.split('invalidatePhantasiNoteDocsCache()').length - 1, 1)
  })

  it('协同 WS 编辑帧不带正文，连接不跟正文重绑', () => {
    const collab = read('notes/useNoteCollab.ts')
    const send = collab.slice(collab.indexOf('type: \'edit\''))
    assert.match(collab, /liveRef/)
    const connectionDeps = collab.match(/\}, \[([^\]]+)\]\)/)?.[1] ?? ''
    assert.match(connectionDeps, /cloudId/)
    assert.doesNotMatch(connectionDeps, /fields|contentMd|title|topic|cover/)
    assert.doesNotMatch(collab, /COLLAB_BODY_FRAME_BYTES/)
    assert.doesNotMatch(send, /content_md/)
    assert.doesNotMatch(collab, /setInterval\([^)]{0,80}200\)/)
    assert.match(send, /type: 'edit'/)
    assert.match(send, /cursor:/)
  })

  it('草稿列表走 pageData，工作台读这份缓存', () => {
    const pageData = read('pageData.ts')
    assert.match(pageData, /NOTE_DOCS_CACHE_KEY/)
    assert.match(pageData, /export async function loadNoteDocs/)
    assert.match(pageData, /phantasiApi\.listNoteDocs/)
    assert.match(read('usePhantasiWorkbench.ts'), /loadNoteDocs/)
  })

  it('创作侧皮不自己拉文档 API', () => {
    for (const name of [
      'skin/PhantasiWorkbenchNotes.tsx',
      'skin/PhantasiWorkbenchComments.tsx',
      'skin/PhantasiWorkbenchMedia.tsx',
      'skin/PhantasiWorkbenchReviews.tsx',
      'skin/PhantasiWorkbenchIo.tsx',
      'skin/PhantasiWorkbenchChrome.tsx',
    ]) {
      const src = read(name)
      assert.doesNotMatch(src, /from ['"].*phantasiApi['"]/, name)
      assert.doesNotMatch(src, /from ['"].*pageData['"]/, name)
    }
  })

  it('发布/定时/取消定时前端带 revision', () => {
    const publish = read('notes/useNotePublish.ts')
    const api = read('../../services/phantasiApi.ts')
    assert.match(publish, /revision: revisionRef\.current/)
    assert.match(publish, /publishNoteDoc/)
    assert.match(publish, /scheduleNoteDoc/)
    const unschedulePayload = publish.match(/unscheduleNoteDoc\(cloudId, \{([^}]+)\}/)?.[1] ?? ''
    assert.match(unschedulePayload, /revision: revisionRef\.current/)
    assert.match(unschedulePayload, /client_request_id:/)
    assert.match(fnBody(api, 'unscheduleNoteDoc'), /JSON\.stringify\(req\)/)
  })

  it('空草稿关闭会删云端稿', () => {
    const publish = read('notes/useNotePublish.ts')
    assert.match(publish, /docStatus === 'draft'/)
    assert.match(publish, /!title\.trim\(\)/)
    assert.match(publish, /!contentMd\.trim\(\)/)
    assert.match(publish, /deleteNoteDoc\(cloudId\)/)
    assert.match(publish, /labels\.deleteFailed/)
    assert.match(publish, /return/)
  })

  it('工作台搜索不读 content_md，列表封面走 image', () => {
    const workbench = read('logic/workbench.ts')
    const start = workbench.indexOf('function noteSearchHaystack')
    const joined = workbench.slice(workbench.indexOf('return [', start), start + 500)
    assert.doesNotMatch(joined, /doc\.content_md/)
    assert.match(joined, /doc\.excerpt/)
    assert.match(workbench, /workbenchNoteCover/)
  })

  it('工作台批量删除列出失败项', () => {
    const workbench = read('usePhantasiWorkbench.ts')
    assert.match(workbench, /failed/)
    assert.match(workbench, /names/)
    assert.match(workbench, /#\$\{doc\.id\}/)
    assert.match(workbench, /return false/)
    const notes = read('skin/PhantasiWorkbenchNotes.tsx')
    assert.match(notes, /ok !== false/)
  })

  it('新建分类失败不静默', () => {
    const format = read('notes/useNoteEditorFormat.ts')
    assert.match(format, /createCategory/)
    assert.match(format, /errorSaveFailed/)
    assert.match(format, /showNoteNotice\(\s*userFacingError\(err, t\.phantasi\.errorSaveFailed\)/)
    assert.doesNotMatch(format, /createCategory\(\{ name: next \}\)\.catch\(\(\) => \{\}\)/)
  })

  it('生产路径不打 console.log / console.debug', () => {
    const walk = (rootDir: string): string[] =>
      readdirSync(rootDir).flatMap((name) => {
        const next = join(rootDir, name)
        if (statSync(next).isDirectory()) return walk(next)
        if (!/\.(ts|tsx)$/.test(name) || name.includes('.test.')) return []
        return [next]
      })
    const files = [
      ...walk(dir),
      join(dir, '../../services/phantasiApi.ts'),
    ]
    for (const file of files) {
      const src = readFileSync(file, 'utf8')
      assert.doesNotMatch(src, /console\.log\(/, file)
      assert.doesNotMatch(src, /console\.debug\(/, file)
    }
  })

  it('升级说明给用户地址、不写 301，brew:write 标成历史', () => {
    const notes = readRoot('docs/development/UPGRADE_NOTES.md')
    assert.match(notes, /\/blog\/posts\/\{slug\}\//)
    assert.match(notes, /\/api\/phantasi\/notes\.xml/)
    assert.match(notes, /\/phantasi\/articles\/\{id\}/)
    assert.match(notes, /无 301/)
    assert.match(notes, /历史权限名 `brew:write`/)
    assert.match(notes, /phantasi:write/)
    assert.doesNotMatch(notes, /仍然有效的 `ui:theme`、`media:control`、`brew:write`/)
    assert.doesNotMatch(notes, /Redirect 301/)
    const spa = readRoot('frontend/scripts/vite/spaFallback.mjs')
    assert.doesNotMatch(spa, /status:\s*301/)
    assert.doesNotMatch(spa, /\/brew/)
  })

  it('编辑器按会话拆开，笔记导入失败要让人看见', () => {
    const editor = read('notes/NoteEditor.tsx')
    const session = read('notes/useNoteEditorSession.ts')
    const preview = read('notes/useNoteEditorPreview.ts')
    assert.match(editor, /useNoteEditorSession/)
    assert.match(editor, /NoteEditorView/)
    assert.match(session, /useNoteEditorOpen/)
    assert.match(session, /useNoteCloudSave/)
    assert.match(session, /useNotePublish/)
    assert.match(session, /useNoteVisual/)
    assert.doesNotMatch(editor, /openNoteCloudDoc/)
    assert.doesNotMatch(session, /openNoteCloudDoc/)
    assert.match(session, /useNoteEditorAuthors/)
    assert.match(session, /useNoteEditorFormat/)
    assert.match(session, /useNoteEditorPreview/)
    assert.match(session, /useNoteEditorSidecar/)
    assert.match(preview, /noteEditorCaret/)
    assert.match(read('notes/useNoteEditorSidecar.ts'), /showNoteNotice/)
    assert.match(read('notes/useNoteEditorSidecar.ts'), /loadFailed/)
    assert.match(session, /errorLoadFailed/)
    assert.doesNotMatch(session, /\.catch\(\(\) => \{\}\)/)
    assert.match(read('manager/contentIo/contentIo.ts'), /showPhantasiError/)
    assert.match(
      read('manager/contentIo/contentIo.ts'),
      /skipped \+= 1\s*void showPhantasiError/,
    )
  })
})
