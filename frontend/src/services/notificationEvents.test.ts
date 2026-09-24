import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import {
  isNotificationEventKey,
  MEROPE_EVENT_PREFIX,
  NOTIFICATION_EVENT_KEYS,
  NOTIFICATION_EVENT_SOURCES,
  NOTIFICATION_SOURCE_KEYS,
  NotificationAction,
  notificationEventKeyOf,
} from './notificationEvents.ts'

// The backend checks its `NotificationEventKey` enum against the same file.
const SPEC = JSON.parse(
  readFileSync(
    new URL('../../../shared/notification_events.json', import.meta.url),
    'utf8',
  ),
) as {
  sources: string[]
  events: { key: string; source: string }[]
  actions: string[]
}

describe('notification event catalog', () => {
  it('matches the shared contract exactly, in order', () => {
    assert.deepEqual([...NOTIFICATION_SOURCE_KEYS], SPEC.sources)
    assert.deepEqual(
      NOTIFICATION_EVENT_KEYS.map((key) => ({
        key,
        source: NOTIFICATION_EVENT_SOURCES[key],
      })),
      SPEC.events,
    )
    assert.deepEqual(Object.values(NotificationAction), SPEC.actions)
  })

  it('has one source entry per key and nothing else', () => {
    assert.deepEqual(
      Object.keys(NOTIFICATION_EVENT_SOURCES).toSorted(),
      [...NOTIFICATION_EVENT_KEYS].toSorted(),
    )
  })

  it('keeps every catalogued Merope event under the Merope prefix', () => {
    const merope = NOTIFICATION_EVENT_KEYS.filter((key) =>
      key.startsWith(MEROPE_EVENT_PREFIX),
    )
    assert.ok(merope.length > 0)
    for (const key of merope) {
      assert.equal(NOTIFICATION_EVENT_SOURCES[key], 'agent', key)
    }
  })

  it('only recognises catalogued keys', () => {
    assert.equal(isNotificationEventKey('federation.delivery_failed'), true)
    assert.equal(isNotificationEventKey('federation.not_in_catalog'), false)
    assert.equal(isNotificationEventKey('toString'), false)
    assert.equal(
      notificationEventKeyOf({ event_key: 'platform.sync.failed' }),
      'platform.sync.failed',
    )
    assert.equal(notificationEventKeyOf({ event_key: 42 }), undefined)
    assert.equal(notificationEventKeyOf(null), undefined)
  })
})
