import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

const loadHooks = [
  'usePhantasiWorkbench.ts',
  'usePhantasiCategories.ts',
]

describe('phantasi page hooks 边界', () => {
  it('留下的 hook 不进口 skin / ui / manager', () => {
    for (const name of loadHooks) {
      const src = readFileSync(join(dir, name), 'utf8')
      assert.doesNotMatch(src, /from ['"]\.\/skin/)
      assert.doesNotMatch(src, /from ['"]\.\/ui/)
      assert.doesNotMatch(src, /from ['"]\.\/manager/)
    }
  })

  it('分类 hook 改目录并改写笔记，不进口 manager / 编辑器', () => {
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

  it('工作室按需拆开编辑器', () => {
    const studio = readFileSync(join(dir, '../../views/Studio.tsx'), 'utf8')
    assert.match(studio, /lazy\(\(\) => import\(['"][^'"]*NoteEditor['"]\)\)/)
    assert.doesNotMatch(studio, /import NoteEditor from/)
  })
})
