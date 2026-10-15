import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { parseFeedTopicCards } from './feedTopicCards.ts'

describe('parseFeedTopicCards', () => {
  it('读数组或 cards 对象，去空白去重', () => {
    assert.deepEqual(parseFeedTopicCards(['Rust', ' AI ', 'Rust', '']), [
      'Rust',
      'AI',
    ])
    assert.deepEqual(parseFeedTopicCards({ cards: ['Rust', 1, 'AI'] }), [
      'Rust',
      'AI',
    ])
    assert.deepEqual(parseFeedTopicCards(null), [])
    assert.deepEqual(parseFeedTopicCards(true), [])
  })
})
