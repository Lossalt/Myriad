import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('BatchCategoryPick', () => {
  it('没勾选也能打开下拉，空名单不把控件整段禁用', () => {
    const src = readFileSync(join(dir, 'BatchCategoryPick.tsx'), 'utf8')
    const notes = readFileSync(join(dir, 'skin/PhantasiWorkbenchNotes.tsx'), 'utf8')
    const list = readFileSync(
      join(dir, '../settings/ManagedList.tsx'),
      'utf8',
    )
    const chrome = readFileSync(join(dir, 'useListSelection.tsx'), 'utf8')
    assert.match(src, /SettingTitleSelect/)
    assert.match(src, /searchable/)
    assert.doesNotMatch(src, /names\.length === 0/)
    assert.doesNotMatch(notes, /notesSelect\.picked === 0/)
    assert.doesNotMatch(notes, /sourceSelect\.picked === 0/)
    assert.match(list, /managed-list-filter-tools/)
    assert.match(list, /managed-list-filter-tools-row/)
    assert.match(list, /managed-list-filter-expanded/)
    assert.match(list, /toolbarExtra/)
    assert.match(list, /SettingTitleTag/)
    assert.match(list, /managed-list-filter-tag/)
    assert.doesNotMatch(
      list,
      /choice-segmented choice-segmented--sm is-flex managed-list-filters/,
    )
    assert.match(chrome, /slot: 'lead'/)
    assert.match(chrome, /slot: 'expanded'/)
  })
})
