import type { AppNotification } from '../services/notificationApi'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  createNotificationHistoryJournal,
  journalCleared,
  journalLive,
  journalRemoved,
  mergeNotificationHistory,
} from './notificationHistory'

function note(id: string, body = id): AppNotification {
  return {
    id,
    notification_type: 'system',
    priority: 'normal',
    title: id,
    body,
    user_id: 1,
    created_at: '2026-01-01T00:00:00Z',
    read: false,
  } as AppNotification
}

const ids = (items: readonly AppNotification[]) => items.map(n => n.id)

test('history keeps SSE items that arrived while it was in flight', () => {
  const journal = createNotificationHistoryJournal()
  // Stale snapshot from before the request: replaced by history.
  const current = [note('sse-new'), note('task', 'progress 80%'), note('old')]
  journalLive(journal, 'sse-new')
  journalLive(journal, 'task')
  const history = [note('task', 'progress 10%'), note('h1'), note('h2')]

  const merged = mergeNotificationHistory(current, history, journal, 100)

  assert.deepEqual(ids(merged), ['sse-new', 'task', 'h1', 'h2'])
  assert.equal(merged[1].body, 'progress 80%')
})

test('history does not resurrect items removed while it was in flight', () => {
  const journal = createNotificationHistoryJournal()
  journalRemoved(journal, 'h1')
  const merged = mergeNotificationHistory([], [note('h1'), note('h2')], journal, 100)
  assert.deepEqual(ids(merged), ['h2'])
})

test('a clear during the request keeps only what arrived after it', () => {
  const journal = createNotificationHistoryJournal()
  journalLive(journal, 'before-clear')
  journalCleared(journal)
  journalLive(journal, 'after-clear')
  const merged = mergeNotificationHistory([note('after-clear')], [note('h1')], journal, 100)
  assert.deepEqual(ids(merged), ['after-clear'])
})

test('without in-flight changes history replaces the list and respects the cap', () => {
  const merged = mergeNotificationHistory(
    [note('stale')],
    [note('h1'), note('h2'), note('h3')],
    createNotificationHistoryJournal(),
    2,
  )
  assert.deepEqual(ids(merged), ['h1', 'h2'])
})
