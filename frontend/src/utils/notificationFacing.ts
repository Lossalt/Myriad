import type { AppNotification } from '../services/notificationApi'
import { currentCopy, formatCurrent } from '../i18n/localeCopy'
import {
  NotificationEvent,
  notificationEventKeyOf,
} from '../services/notificationEvents'
import {
  isInternalDump,
  isUselessErrorText,
} from './uselessErrorText'

const DEFAULT_TAPP_TITLES = new Set([
  'App notification',
  'Tapp notification',
  'Tapp 通知',
  'Tapp 알림',
  'Notification Tapp',
  'Tapp-Benachrichtigung',
])

const SCHEDULED_TAPP_TITLE = /^(?:定时任务失败|Scheduled task failed|A scheduled task failed)$/i

function fill(
  template: string,
  params: Record<string, string | number>,
): string {
  return formatCurrent(template, params)
}

function noticeLeftover(raw: string, fallback: string): string {
  const t = currentCopy().errors
  const text = raw.replaceAll(/\s+/g, ' ').trim()
  if (!text) return fallback
  const lower = text.toLowerCase()
  if (lower === 'unauthorized') return t.unauthorized
  if (lower === 'forbidden') return t.forbidden
  if (lower === 'not found') return t.notFound
  if (/^维护重试成功$|^Maintenance retry succeeded$/i.test(text)) {
    return t.noticeMcpMaintenanceRetry
  }
  if (/^自动重启成功$|^Auto-restart succeeded$/i.test(text)) {
    return t.noticeMcpAutoRestart
  }
  const colon = text.indexOf(':')
  if (colon >= 0) {
    const rest = text.slice(colon + 1).trim()
    if (isInternalDump(rest) || isUselessErrorText(rest)) {
      const head = text.slice(0, colon).trim()
      return head || fallback
    }
  }
  if (isInternalDump(text) || isUselessErrorText(text)) return fallback
  return text
}

function metaString(
  notification: AppNotification,
  key: string,
): string {
  const value = notification.metadata?.[key]
  return typeof value === 'string' && value.trim() ? value.trim() : ''
}

export function notificationFacingTitle(notification: AppNotification): string {
  const t = currentCopy().errors
  const eventKey = notificationEventKeyOf(notification.metadata)
  const name =
    metaString(notification, 'source_name') ||
    metaString(notification, 'platform') ||
    metaString(notification, 'server_id') ||
    metaString(notification, 'tapp_id')

  switch (eventKey) {
    case NotificationEvent.phantasiSourceError:
      return fill(t.noticePhantasiSourceFailed, { name: name || 'RSS' })
    case NotificationEvent.phantasiNewItems:
      return fill(t.noticePhantasiNewItems, {
        name:
          name ||
          notification.title.replaceAll(/\s*·\s*\d.*$/g, '').trim() ||
          'RSS',
        n:
          typeof notification.metadata?.new_count === 'number'
            ? notification.metadata.new_count
            : 0,
      })
    case NotificationEvent.heartbeatSeoReview:
      return t.noticeSeoReview
    case NotificationEvent.heartbeatSucceeded:
    case NotificationEvent.heartbeatFailed:
      return fill(t.noticeHeartbeatTask, {
        name:
          metaString(notification, 'task_name') ||
          notification.title
            .replaceAll(/^定时任务:\s*/g, '')
            .replaceAll(/^Scheduled task:\s*/gi, '')
            .trim() ||
          'task',
      })
    case NotificationEvent.platformSyncFailed:
      return fill(t.noticePlatformSyncFailed, { name: name || 'Steam' })
    case NotificationEvent.mcpDisconnected:
      return fill(t.noticeMcpFailed, { name: name || 'MCP' })
    case NotificationEvent.mcpConnected:
      return fill(t.noticeMcpConnected, { name: name || 'MCP' })
    case NotificationEvent.agentTaskFailed:
      return t.noticeAgentTaskFailed
    case NotificationEvent.agentTaskCompleted:
      return t.noticeAgentTaskCompleted
    case NotificationEvent.agentTaskCancelled:
      return t.agentTaskCancelled
    case NotificationEvent.agentClarification:
      return t.noticeAgentTaskWaiting
    case NotificationEvent.agentTaskProgress:
      return t.noticeAgentTaskRunning
    case NotificationEvent.updaterSucceeded:
      return t.noticeUpdaterSucceeded
    case NotificationEvent.updaterFailed:
      return t.noticeUpdaterFailed
    case NotificationEvent.updaterNeedsManual:
      return t.noticeUpdaterNeedsManual
    case NotificationEvent.updaterRunning:
      return t.noticeUpdaterRunning
    case NotificationEvent.updaterUnknown:
      return t.noticeUpdaterUnknown
    case NotificationEvent.updaterSubmitted:
      return t.noticeUpdaterSubmitted
    case NotificationEvent.federationDomainRevoked:
      return fill(t.noticeFederationRevoked, {
        name: metaString(notification, 'target_domain') || name || 'remote',
      })
    case NotificationEvent.federationNewFollower:
      return t.noticeNewFollower
    case NotificationEvent.federationFollowAccepted:
      return t.noticeFollowAccepted
    case NotificationEvent.federationChannelInvite:
      return t.noticeChannelInvite
    case NotificationEvent.federationRoomInvite:
      return t.noticeRoomInvite
    case NotificationEvent.federationRoomInviteAccepted:
      return t.noticeRoomInviteAccepted
    case NotificationEvent.federationChannelAccepted:
      return t.noticeChannelAccepted
    case NotificationEvent.federationDeliveryFailed:
      return t.noticeDeliveryFailed
    case NotificationEvent.skillPruned:
      return fill(t.noticeSkillPruned, {
        name: metaString(notification, 'skill_id') || name,
      })
    case NotificationEvent.skillImproved:
      return fill(t.noticeSkillImproved, {
        name: metaString(notification, 'skill_id') || name,
      })
    case NotificationEvent.skillChanged:
      return fill(t.noticeSkillChanged, {
        name: metaString(notification, 'skill_id') || name,
      })
    default:
      break
  }

  const raw = notification.title || ''
  if (notification.notification_type === 'tapp_notification') {
    if (!raw || DEFAULT_TAPP_TITLES.has(raw)) return t.noticeTapp
    if (SCHEDULED_TAPP_TITLE.test(raw)) return t.noticeScheduleFailed
    return raw
  }
  if (notification.notification_type === 'federation_message') return raw
  const leftoverPhantasiNew = raw.match(/^(.+) · (\d+) 篇新内容$/)
  if (leftoverPhantasiNew) {
    return fill(t.noticePhantasiNewItems, {
      name: leftoverPhantasiNew[1],
      n: leftoverPhantasiNew[2],
    })
  }
  const leftoverHeartbeat = raw.match(/^定时任务:\s*(\S.*)$/)
  if (leftoverHeartbeat) {
    return fill(t.noticeHeartbeatTask, { name: leftoverHeartbeat[1] })
  }
  if (raw.includes('连续抓取失败')) {
    return fill(t.noticePhantasiSourceFailed, { name: name || raw.replaceAll(/连续抓取失败/g, '').trim() || 'RSS' })
  }
  if (raw.includes('自动刷新失败')) {
    return fill(t.noticePlatformSyncFailed, { name: name || raw.replaceAll(/自动刷新失败/g, '').trim() })
  }
  if (raw.includes('连接失败') && raw.includes('MCP')) {
    return fill(t.noticeMcpFailed, { name: name || 'MCP' })
  }
  if (raw.includes('定时任务失败')) return t.noticeScheduleFailed
  if (/^任务失败$|^任务执行失败$|^前端任务执行失败$|^The task failed$/.test(raw)) {
    return t.noticeAgentTaskFailed
  }
  if (/^任务完成$|^任务已完成$|^The task finished$/.test(raw)) {
    return t.noticeAgentTaskCompleted
  }
  if (/^任务已取消$|^The task was cancelled$/.test(raw)) {
    return t.agentTaskCancelled
  }
  if (/任务等待通道已断开|^The wait channel closed$/.test(raw)) {
    return t.waitChannelClosed
  }
  if (/等待用户输入已超时|^Waiting for input timed out/.test(raw)) {
    return t.waitInputTimeout
  }
  if (/任务状态已不可用|^The task is no longer available$/.test(raw)) {
    return t.taskUnavailable
  }
  if (/任务等待你的回答|The task needs your reply/.test(raw)) {
    return t.noticeAgentTaskWaiting
  }
  if (/Arael 正在执行任务|^Arael is working$|^Agent is working$/.test(raw)) {
    return t.noticeAgentTaskRunning
  }
  if (raw.includes('系统更新任务失败')) return t.noticeUpdaterFailed
  if (raw.includes('系统更新需要人工')) return t.noticeUpdaterNeedsManual
  if (raw.includes('Tapp 通知')) return t.noticeTapp
  if (raw.includes('联邦关系已解除')) {
    return fill(t.noticeFederationRevoked, {
      name: metaString(notification, 'target_domain') || 'remote',
    })
  }
  if (raw.includes('新的关注者')) return t.noticeNewFollower
  if (raw.includes('关注已通过')) return t.noticeFollowAccepted
  if (raw.includes('新的私信请求')) return t.noticeChannelInvite
  if (raw.includes('群组邀请已接受')) return t.noticeRoomInviteAccepted
  if (raw.includes('群组邀请')) return t.noticeRoomInvite
  if (raw.includes('私信通道已建立')) return t.noticeChannelAccepted
  if (raw.includes('联邦投递失败')) return t.noticeDeliveryFailed
  if (raw.includes('技能已自动淘汰')) {
    return fill(t.noticeSkillPruned, {
      name: metaString(notification, 'skill_id') || name,
    })
  }
  if (raw.includes('技能已自动改进')) {
    return fill(t.noticeSkillImproved, {
      name: metaString(notification, 'skill_id') || name,
    })
  }
  if (/feed failed repeatedly/i.test(raw)) {
    return fill(t.noticePhantasiSourceFailed, { name: name || 'RSS' })
  }
  if (/auto-refresh failed/i.test(raw)) {
    return fill(t.noticePlatformSyncFailed, { name: name || 'Steam' })
  }
  if (/scheduled task failed|a scheduled task failed/i.test(raw)) {
    return t.noticeScheduleFailed
  }
  if (/system update failed/i.test(raw)) return t.noticeUpdaterFailed
  if (/system update needs/i.test(raw)) return t.noticeUpdaterNeedsManual
  if (!raw || isUselessErrorText(raw)) return t.noticeTapp
  const mapped = noticeLeftover(raw, t.noticeTapp)
  return mapped === raw ? raw : mapped
}

export function notificationFacingBody(notification: AppNotification): string {
  const t = currentCopy().errors
  const eventKey = notificationEventKeyOf(notification.metadata)
  const actor = metaString(notification, 'actor_label')
  const room = metaString(notification, 'room_name')
  if (eventKey === NotificationEvent.federationDomainRevoked) {
    const count = notification.metadata?.cancelled_deliveries
    return fill(t.noticeFederationRevokedBody, {
      name: metaString(notification, 'target_domain') || 'remote',
      count: typeof count === 'number' ? count : Number(count ?? 0),
    })
  }
  if (eventKey === NotificationEvent.federationNewFollower) {
    return fill(t.noticeNewFollowerBody, { name: actor || 'someone' })
  }
  if (eventKey === NotificationEvent.federationFollowAccepted) {
    return fill(t.noticeFollowAcceptedBody, { name: actor || 'someone' })
  }
  if (eventKey === NotificationEvent.federationChannelInvite) {
    return fill(t.noticeChannelInviteBody, { name: actor || 'someone' })
  }
  if (eventKey === NotificationEvent.federationRoomInvite) {
    return room
      ? fill(t.noticeRoomInviteNamedBody, { name: actor || 'someone', room })
      : fill(t.noticeRoomInviteBody, { name: actor || 'someone' })
  }
  if (eventKey === NotificationEvent.federationRoomInviteAccepted) {
    return room
      ? fill(t.noticeRoomInviteAcceptedNamedBody, {
          name: actor || 'someone',
          room,
        })
      : fill(t.noticeRoomInviteAcceptedBody, { name: actor || 'someone' })
  }
  if (eventKey === NotificationEvent.federationChannelAccepted) {
    return fill(t.noticeChannelAcceptedBody, { name: actor || 'someone' })
  }
  if (eventKey === NotificationEvent.federationDeliveryFailed) {
    return fill(t.noticeDeliveryFailedBody, {
      name: metaString(notification, 'target_domain') || 'remote',
    })
  }
  if (eventKey === NotificationEvent.heartbeatSeoReview) {
    return notification.body
  }
  if (eventKey === NotificationEvent.skillImproved) {
    return t.noticeSkillImprovedBody
  }
  if (eventKey === NotificationEvent.skillPruned) {
    return fill(t.noticeSkillPrunedBody, {
      name: metaString(notification, 'skill_id') || 'skill',
    })
  }
  if (
    eventKey === NotificationEvent.phantasiNewItems &&
    (!notification.body ||
      /^发现 \d+ 篇新内容$/.test(notification.body) ||
      /^\d+ new items found$/i.test(notification.body))
  ) {
    const n = notification.metadata?.new_count
    return fill(t.noticePhantasiNewItemsBody, {
      n: typeof n === 'number' ? n : 0,
    })
  }
  const messageType = metaString(notification, 'message_type')
  if (messageType === 'image' || /^📷 图片$|^Photo$/.test(notification.body)) {
    return t.noticePreviewPhoto
  }
  if (
    messageType === 'file' ||
    messageType === 'file-meta' ||
    /^📎 文件$|^File$/.test(notification.body)
  ) {
    return t.noticePreviewFile
  }
  if (messageType === 'system' || /^系统消息$|^System message$/.test(notification.body)) {
    return t.noticePreviewSystem
  }
  if (
    /^🔒 加密消息$|^Encrypted message$/.test(notification.body)
  ) {
    return t.noticePreviewEncrypted
  }
  if (/^新消息$|^New message$/.test(notification.body)) {
    return t.noticePreviewNew
  }
  if (notification.notification_type === 'federation_message') {
    return notification.body
  }
  if (notification.notification_type === 'tapp_notification') {
    return SCHEDULED_TAPP_TITLE.test(notification.title)
      ? noticeLeftover(notification.body, notification.body)
      : notification.body
  }
  const leftoverPhantasiBody = notification.body.match(/^发现 (\d+) 篇新内容$/)
  if (leftoverPhantasiBody) {
    return fill(t.noticePhantasiNewItemsBody, { n: Number(leftoverPhantasiBody[1]) })
  }
  if (!notification.body.trim()) return ''
  return noticeLeftover(notification.body, notification.body)
}
