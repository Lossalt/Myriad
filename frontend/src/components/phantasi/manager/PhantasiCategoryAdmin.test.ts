import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('PhantasiCategoryAdmin', () => {
  it('分类管理不进口皮，笔记和订阅各一页', () => {
    const src = readFileSync(join(dir, 'PhantasiCategoryAdmin.tsx'), 'utf8')
    const page =
      readFileSync(join(dir, '../../../views/Studio.tsx'), 'utf8') +
      readFileSync(join(dir, '../../studio/StudioShell.tsx'), 'utf8')
    assert.match(src, /workbench-note-categories/)
    assert.match(src, /workbench-source-categories/)
    assert.match(src, /workbenchCategoryNoteUsage/)
    assert.match(src, /workbenchCategorySourceUsage/)
    assert.match(src, /workbenchCategoryLocked/)
    assert.match(src, /noteCategoryNew/)
    assert.match(src, /id: '__new__'/)
    assert.doesNotMatch(src, /formOpen/)
    assert.doesNotMatch(src, /formTitle/)
    assert.doesNotMatch(src, /phantasi-workbench__category-create/)
    assert.match(src, /renderHit/)
    assert.match(src, /onOpen/)
    assert.match(src, /sourceMatchesCategory/)
    assert.match(src, /noteMatchesCategory/)
    assert.match(src, /workbenchCategoryRename/)
    assert.match(src, /workbenchCategoryDeleteNotesConfirm/)
    assert.match(src, /workbenchCategoryDeleteSourcesConfirm/)
    assert.doesNotMatch(src, /noteCategoryAll/)
    assert.doesNotMatch(src, /未分类/)
    assert.doesNotMatch(src, /from ['"]\.\.\/skin\//)
    assert.doesNotMatch(src, /PhantasiWorkbenchHome/)
    assert.doesNotMatch(src, /from ['"].*phantasiApi['"]/)
    assert.match(page, /PhantasiCategoryAdmin/)
    assert.match(page, /usePhantasiCategories/)
    assert.match(page, /page="notes"/)
    assert.match(page, /categories\.noteRows/)
    assert.doesNotMatch(page, /page="sources"/)
  })
})
