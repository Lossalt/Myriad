import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

const loadHooks = [
  'usePhantasiSources.ts',
  'usePhantasiItems.ts',
  'usePhantasiStarred.ts',
  'usePhantasiItemActions.ts',
  'usePhantasiItemRoute.ts',
  'usePhantasiBoardRoute.ts',
  'usePhantasiAgentOpen.ts',
  'usePhantasiSurface.ts',
  'usePhantasiNotes.ts',
  'usePhantasiSeo.ts',
  'usePhantasiNavExpand.ts',
  'useBoardPage.ts',
  'usePhantasiWorkbench.ts',
  'usePhantasiCategories.ts',
]

describe('phantasi page hooks 边界', () => {
  it('加载 hook 不进口 skin / ui / manager', () => {
    for (const name of loadHooks) {
      const src = readFileSync(join(dir, name), 'utf8')
      assert.doesNotMatch(src, /from ['"]\.\/skin/)
      assert.doesNotMatch(src, /from ['"]\.\/ui/)
      assert.doesNotMatch(src, /from ['"]\.\/manager/)
    }
  })

  it('分类 hook 改目录并改写笔记/订阅，不进口 manager / 编辑器', () => {
    const src = readFileSync(join(dir, 'usePhantasiCategories.ts'), 'utf8')
    assert.match(src, /getCategories/)
    assert.match(src, /createCategory/)
    assert.match(src, /updateCategory/)
    assert.match(src, /deleteCategory/)
    assert.match(src, /updateNoteDocTopic/)
    assert.match(src, /resolveAddCategoryPart/)
    assert.match(src, /formatCategoryFullNotice/)
    assert.match(src, /renameCategoryPart/)
    assert.match(src, /removeCategoryPart/)
    assert.match(src, /\bassign\b/)
    assert.match(src, /categoryFull/)
    assert.doesNotMatch(src, /phantasiApi\.updateNote\(/)
    assert.match(src, /updateSource/)
    assert.doesNotMatch(src, /NoteEditor/)
    assert.doesNotMatch(src, /from ['"]\.\/manager/)
  })

  it('工作台 hook 不进口编辑器正文，只调文档和媒体 API', () => {
    const src = readFileSync(join(dir, 'usePhantasiWorkbench.ts'), 'utf8')
    assert.match(src, /loadNoteDocs/)
    assert.match(src, /deleteNoteDoc/)
    assert.match(src, /removeNotes/)
    assert.match(src, /mediaApi/)
    assert.doesNotMatch(src, /NoteEditor/)
    assert.doesNotMatch(src, /useReaderSettings/)
  })

  it('工作台 hook 只在工作台 lane 挂载', () => {
    const page = readFileSync(join(dir, '../../views/Phantasi.tsx'), 'utf8')
    const lane = readFileSync(join(dir, 'PhantasiWorkbenchLane.tsx'), 'utf8')
    assert.doesNotMatch(page, /usePhantasiWorkbench\(/)
    assert.doesNotMatch(page, /usePhantasiCategories\(/)
    assert.doesNotMatch(page, /useNoteTransfer\(/)
    assert.doesNotMatch(page, /usePipack\(/)
    assert.match(page, /<PhantasiWorkbenchLane/)
    assert.match(lane, /usePhantasiWorkbench\(/)
    assert.match(lane, /usePhantasiCategories\(/)
    assert.match(lane, /useNoteTransfer\(/)
  })

  it('阅读器、编辑器、工作台按需拆包，笔记墙不跟 sources 身份重拉', () => {
    const page = readFileSync(join(dir, '../../views/Phantasi.tsx'), 'utf8')
    const board = readFileSync(join(dir, 'useBoardPage.ts'), 'utf8')
    const sources = readFileSync(join(dir, 'usePhantasiSources.ts'), 'utf8')
    const studio = readFileSync(join(dir, '../../views/Studio.tsx'), 'utf8')
    assert.match(page, /lazy\(\(\) => import\(['"][^'"]*PhantasiReader['"]\)\)/)
    assert.match(studio, /lazy\(\(\) => import\(['"][^'"]*NoteEditor['"]\)\)/)
    assert.doesNotMatch(page, /NoteEditor/)
    assert.match(page, /lazy\(\(\) => import\(['"][^'"]*PhantasiWorkbenchLane['"]\)\)/)
    assert.doesNotMatch(page, /import PhantasiReader from/)
    assert.doesNotMatch(studio, /import NoteEditor from/)
    assert.doesNotMatch(page, /import PhantasiWorkbenchLane from/)
    assert.match(page, /<Suspense/)
    assert.match(page, /journalSourceScope\(location\.pathname\)/)
    assert.match(board, /noteSourceStamp/)
    assert.doesNotMatch(board, /\[board, key, sources\]/)
    assert.doesNotMatch(sources, /flags\.project\(/)
    assert.doesNotMatch(sources, /flagsRevision/)
    const grid = readFileSync(join(dir, 'PhantasiSourceGrid.tsx'), 'utf8')
    assert.match(grid, /loadNoteDocs/)
    assert.doesNotMatch(grid, /\[board, isAdmin, notes, docsEpoch\]/)
    assert.match(grid, /\[board, isAdmin, docsEpoch\]/)
  })

  it('订阅轨手势中按住拼轨，松手再 tick', () => {
    const src = readFileSync(join(dir, 'useBoardPage.ts'), 'utf8')
    assert.match(src, /quietRef/)
    assert.match(src, /pendingBumpRef/)
    assert.match(src, /holdStories/)
    assert.match(src, /releaseStories/)
    assert.match(src, /if \(quietRef\.current\)/)
  })
})
