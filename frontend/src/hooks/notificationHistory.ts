import type { AppNotification } from '../services/notificationApi'

/**
 * What changed locally (SSE or the user's own delete/clear) while a history
 * request was in flight. Those changes are newer than the history snapshot.
 */
export interface NotificationHistoryJournal {
  live: Set<string>
  removed: Set<string>
  cleared: boolean
}

export function createNotificationHistoryJournal(): NotificationHistoryJournal {
  return { live: new Set(), removed: new Set(), cleared: false }
}

export function journalLive(journal: NotificationHistoryJournal, id: string): void {
  journal.live.add(id)
  journal.removed.delete(id)
}

export function journalRemoved(journal: NotificationHistoryJournal, id: string): void {
  journal.live.delete(id)
  journal.removed.add(id)
}

export function journalCleared(journal: NotificationHistoryJournal): void {
  journal.live.clear()
  journal.removed.clear()
  journal.cleared = true
}

/**
 * Apply a history snapshot without losing what arrived while it was in flight:
 * live items keep their newer copy and stay on top, removed ids stay gone, and
 * a clear during the request leaves only what arrived after it.
 */
export function mergeNotificationHistory(
  current: readonly AppNotification[],
  history: readonly AppNotification[],
  journal: NotificationHistoryJournal,
  max: number,
): AppNotification[] {
  const live = current.filter(n => journal.live.has(n.id))
  if (journal.cleared) return live.slice(0, max)
  const rest = history.filter(n => !journal.live.has(n.id) && !journal.removed.has(n.id))
  return [...live, ...rest].slice(0, max)
}
