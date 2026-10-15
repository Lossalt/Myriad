import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  canUseCategoryName,
  categoryMatchesPage,
  categoryUsedOnOtherPage,
  collectWorkbenchCategoryPageRows,
  collectWorkbenchCategoryRows,
  formatCategoryFullNotice,
  noteMatchesCategory,
  pickerCategoryName,
  removeCategoryPart,
  renameCategoryPart,
  resolveAddCategoryPart,
  samePickerCategory,
} from './categories.ts'

describe('resolveAddCategoryPart', () => {
  it('空的可加；普通分类也能再叠一个，满两项或已有则跳过', () => {
    assert.deepEqual(resolveAddCategoryPart(null, '工程'), {
      kind: 'next',
      value: '工程',
    })
    assert.deepEqual(resolveAddCategoryPart('工程', '工程'), {
      kind: 'skip',
      reason: 'duplicate',
    })
    assert.deepEqual(resolveAddCategoryPart('工程', '旅行'), {
      kind: 'next',
      value: '工程, 旅行',
    })
    assert.deepEqual(resolveAddCategoryPart('友情链接', '工程'), {
      kind: 'next',
      value: '友情链接, 工程',
    })
    assert.deepEqual(resolveAddCategoryPart('友情链接, 工程', '生活'), {
      kind: 'skip',
      reason: 'full',
    })
    assert.deepEqual(resolveAddCategoryPart('friend_links', '工程'), {
      kind: 'next',
      value: 'friend_links, 工程',
    })
    assert.equal(
      formatCategoryFullNotice('{name}分类已满', '春天的草稿'),
      '春天的草稿分类已满',
    )
  })
})

describe('renameCategoryPart / removeCategoryPart', () => {
  it('只改命中的那一段，删空则清空', () => {
    assert.equal(
      renameCategoryPart('友情链接, 工程', '工程', '技术'),
      '友情链接, 技术',
    )
    assert.equal(renameCategoryPart('工程', '生活', '技术'), '工程')
    assert.equal(removeCategoryPart('友情链接, 工程', '工程'), '友情链接')
    assert.equal(removeCategoryPart('工程', '工程'), '')
    assert.equal(removeCategoryPart(null, '工程'), null)
  })
})

describe('collectWorkbenchCategoryRows', () => {
  it('合并目录、笔记和订阅，预置分类锁住并排后', () => {
    const rows = collectWorkbenchCategoryRows(
      [{ id: 9, name: '旅行' }],
      [{ topic: '旅行' }, { topic: '随笔' }, { topic: null }],
      [
        { category: '工程, 旅行' },
        { category: 'friend_links' },
        { category: 'mine' },
      ],
    )
    assert.deepEqual(
      rows.map((row) => row.name),
      ['工程', '旅行', '随笔', '我', '友情链接'],
    )
    const travel = rows.find((row) => row.name === '旅行')
    assert.equal(travel?.catalogId, 9)
    assert.equal(travel?.noteCount, 1)
    assert.equal(travel?.sourceCount, 1)
    assert.equal(rows.find((row) => row.name === '友情链接')?.locked, true)
    assert.equal(rows.find((row) => row.name === '随笔')?.catalogId, null)
  })

  it('笔记 topic 按段计数，叠了两个都算', () => {
    const rows = collectWorkbenchCategoryRows(
      [],
      [{ topic: '工程, 旅行' }, { topic: '工程' }],
      [],
    )
    assert.equal(rows.find((row) => row.name === '工程')?.noteCount, 2)
    assert.equal(rows.find((row) => row.name === '旅行')?.noteCount, 1)
  })

  it('没有占用时也留下预置分类，编辑订阅看得到的那一对', () => {
    const rows = collectWorkbenchCategoryRows([], [], [])
    assert.deepEqual(
      rows.map((row) => row.name),
      ['我', '友情链接'],
    )
    assert.equal(rows.every((row) => row.locked), true)
  })
})

describe('categoryMatchesPage / canUseCategoryName', () => {
  it('笔记页不看预置，订阅页看预置；预置名不能新建', () => {
    const rows = collectWorkbenchCategoryPageRows(
      [{ id: 9, name: '旅行' }],
      [{ topic: '随笔' }],
      [{ category: '工程' }],
      'notes',
    )
    assert.deepEqual(
      rows.map((row) => row.name),
      ['旅行', '随笔'],
    )
    const sourceRows = collectWorkbenchCategoryPageRows(
      [{ id: 9, name: '旅行' }],
      [{ topic: '随笔' }],
      [{ category: '工程' }],
      'sources',
    )
    assert.deepEqual(
      sourceRows.map((row) => row.name),
      ['工程', '旅行', '我', '友情链接'],
    )
    assert.equal(categoryMatchesPage(sourceRows[0], 'sources'), true)
    assert.equal(
      categoryUsedOnOtherPage(
        {
          name: '随笔',
          catalogId: null,
          noteCount: 2,
          sourceCount: 0,
          locked: false,
        },
        'notes',
      ),
      false,
    )
    assert.equal(canUseCategoryName('旅行', ['旅行']), false)
    assert.equal(canUseCategoryName('新分类', ['旅行']), true)
    assert.equal(canUseCategoryName('友情链接', []), false)
    assert.equal(canUseCategoryName('我', []), false)
  })
})

describe('noteMatchesCategory', () => {
  it('按段认分类名', () => {
    assert.equal(noteMatchesCategory({ topic: '工程, 旅行' }, '工程'), true)
    assert.equal(noteMatchesCategory({ topic: '工程' }, '旅行'), false)
    assert.equal(noteMatchesCategory({ topic: null }, '工程'), false)
  })
})

describe('pickerCategoryName', () => {
  it('别名收成官网名', () => {
    assert.equal(pickerCategoryName('friend-links'), '友情链接')
    assert.equal(pickerCategoryName('own'), '我')
    assert.equal(samePickerCategory('friend_links', '友情链接'), true)
    assert.equal(samePickerCategory('工程', '旅行'), false)
  })
})
