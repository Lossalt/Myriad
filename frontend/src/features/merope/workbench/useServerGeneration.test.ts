import assert from 'node:assert/strict'
import test from 'node:test'
import { generationPending } from './useServerGeneration'

const NOW = Date.parse('2026-10-13T04:00:00Z')

test('a generation is running while its lease is taken and fresh', () => {
  assert.equal(generationPending({ pending: { startedAt: '2026-10-13T03:58:00Z' } }, NOW), true)
})

test('a lease the server would judge lost is not a running generation', () => {
  assert.equal(generationPending({ pending: { startedAt: '2026-10-13T03:40:00Z' } }, NOW), false)
})

test('no lease, or one without a readable start, is nothing running', () => {
  assert.equal(generationPending(null, NOW), false)
  assert.equal(generationPending({ lastCompletedAt: '2026-10-13T03:59:00Z' }, NOW), false)
  assert.equal(generationPending({ pending: { startedAt: 'soon' } }, NOW), false)
  assert.equal(generationPending({ pending: null }, NOW), false)
})
