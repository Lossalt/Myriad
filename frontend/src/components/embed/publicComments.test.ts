import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { publicThread } from './publicComments.ts'

describe('publicThread', () => {
  it('只留下已公开的评论', () => {
    const rows = [
      { id: 1, is_public: true },
      { id: 2, is_public: false },
      { id: 3 },
    ]
    assert.deepEqual(publicThread(rows).map((row) => row.id), [1])
  })
})
