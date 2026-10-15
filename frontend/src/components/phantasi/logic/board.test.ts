import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { sourceMatchesCategory } from './board.ts'
import { makeSource } from './fixtures.ts'

describe('sourceMatchesCategory', () => {
  it('预置分类认别名，普通分类精确匹配', () => {
    assert.equal(
      sourceMatchesCategory(makeSource({ category: 'friend_links' }), '友情链接'),
      true,
    )
    assert.equal(
      sourceMatchesCategory(makeSource({ category: '我,技术' }), '技术'),
      true,
    )
    assert.equal(
      sourceMatchesCategory(makeSource({ category: '技术' }), '我'),
      false,
    )
    assert.equal(sourceMatchesCategory(makeSource({ category: null }), '技术'), false)
  })
})
