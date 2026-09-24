import type { ToastType } from '../components/Toast'
import type { AppNotification } from './notificationApi'
import type {
  NotificationPreferences,
  NotificationSourceKey,
} from './notificationPreferencesApi'
import {
  NOTIFICATION_EVENT_SOURCES,
  notificationEventKeyOf,
} from './notificationEvents'

export type NotificationLocation = 'panel' | 'toast' | 'island' | 'browser'

/**
 * Same rule as the backend `allows()`: a catalogued key owns its source; an
 * unknown or missing key falls back to the notification type, never to a
 * guess from the key's prefix.
 */
export function notificationSourceFor(
  notification: AppNotification,
): NotificationSourceKey {
  const eventKey = notificationEventKeyOf(notification.metadata)
  if (eventKey) return NOTIFICATION_EVENT_SOURCES[eventKey]
  if (notification.notification_type.startsWith('task_')) return 'agent'
  if (notification.notification_type === 'agent_clarification') return 'agent'
  if (notification.notification_type === 'heartbeat_result') return 'heartbeat'
  if (notification.notification_type === 'mcp_server_status') return 'mcp'
  if (notification.notification_type.startsWith('phantasi_')) return 'phantasi'
  if (notification.notification_type === 'tapp_notification') return 'tapp'
  if (notification.notification_type === 'updater_status') return 'updater'
  if (notification.notification_type.startsWith('federation_')) {
    return 'federation'
  }
  return 'system'
}

export function shouldDeliverNotification(
  preferences: NotificationPreferences,
  notification: AppNotification,
  location: NotificationLocation,
): boolean {
  if (!preferences.enabled) return false

  const source = notificationSourceFor(notification)
  if (!preferences.sources[source]) return false

  const eventKey = notificationEventKeyOf(notification.metadata)
  if (eventKey && !preferences.events[eventKey]) return false

  if (!preferences.locations[source]?.[location]) return false
  if (location === 'panel') return true
  return preferences.delivery[location]
}

/** No list/island/toast/push while the Agent panel is open. */
export function shouldSurfaceNotification(
  preferences: NotificationPreferences,
  notification: AppNotification,
  location: NotificationLocation,
  lookingAtAgentPanel: boolean,
): boolean {
  if (lookingAtAgentPanel && notificationSourceFor(notification) === 'agent') {
    return false
  }
  return shouldDeliverNotification(preferences, notification, location)
}

/** No toast while the Agent panel is visible. */
export function shouldEmitNotificationToast(
  preferences: NotificationPreferences,
  notification: AppNotification,
  lookingAtAgentPanel: boolean,
): boolean {
  if (lookingAtAgentPanel) return false
  return shouldDeliverNotification(preferences, notification, 'toast')
}

export function notificationToastType(
  notification: AppNotification,
): ToastType {
  const status = notification.metadata?.status
  const tappType = notification.metadata?.tapp_notification_type
  const failed =
    notification.notification_type === 'task_failed' ||
    notification.notification_type === 'phantasi_source_error' ||
    status === 'failed' ||
    tappType === 'error' ||
    tappType === 'danger'
  if (failed || notification.priority === 'urgent') return 'error'

  const succeeded =
    notification.notification_type === 'task_completed' ||
    status === 'completed' ||
    status === 'succeeded' ||
    tappType === 'success'
  if (succeeded) return 'success'
  if (tappType === 'warning') return 'warning'
  if (notification.priority === 'high') return 'warning'
  return 'info'
}
