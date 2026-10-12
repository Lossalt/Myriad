import type {
  HostLease,
  HostMove,
  SyncDevice,
  SyncDeviceList,
} from '../../services/agent/agentApi'
import type { ManagedListItem, ManagedListStat } from '../settings/ManagedList'
import { LuMonitor, LuPlus } from '@lib/icons'

import { useCallback, useEffect, useMemo, useState } from 'react'
import { useConfigI18n as useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import { ApiError } from '../../services/api'
import { showStickyToast } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import {
  InfoActionCard,
  InputItem,
  ManagedList,
  SettingGroup,
  SettingsButton,
} from '../settings'

/** Devices (a Mac) connected to her on this site. Only the site owner sees it. */
export function AgentDevicesGroup() {
  const { t, format, locale } = useI18n()
  const c = t.config
  const [list, setList] = useState<SyncDeviceList | null>(null)
  const [loading, setLoading] = useState(false)
  const [working, setWorking] = useState(false)
  const [formOpen, setFormOpen] = useState(false)
  const [name, setName] = useState('')
  const [issued, setIssued] = useState<{ name: string; token: string } | null>(
    null,
  )

  const fail = useCallback((error: unknown, fallback: string) => {
    showStickyToast({
      message: userFacingError(error, fallback),
      type: 'error',
      replaceKey: 'config-sync-devices',
    })
  }, [])

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setList(await agentService.listSyncDevices())
    } catch (error) {
      fail(error, c.agentDevicesLoadFailed)
    } finally {
      setLoading(false)
    }
  }, [c.agentDevicesLoadFailed, fail])

  useEffect(() => {
    void load()
  }, [load])

  const add = useCallback(async () => {
    const trimmed = name.trim()
    if (!trimmed) return
    setWorking(true)
    try {
      const { device, token } = await agentService.addSyncDevice(trimmed)
      setIssued({ name: device.name, token })
      setName('')
      setFormOpen(false)
      await load()
    } catch (error) {
      if (error instanceof ApiError && error.code === 'device_limit') {
        fail(error, format(c.agentDevicesLimit, { n: list?.limit ?? 10 }))
      } else {
        fail(error, c.agentDevicesAddFailed)
      }
    } finally {
      setWorking(false)
    }
  }, [c, fail, format, list?.limit, load, name])

  const remove = useCallback(
    async (device: SyncDevice) => {
      setWorking(true)
      try {
        await agentService.removeSyncDevice(device.id)
        await load()
      } catch (error) {
        fail(error, c.agentDevicesRemoveFailed)
      } finally {
        setWorking(false)
      }
    },
    [c.agentDevicesRemoveFailed, fail, load],
  )

  // A move of her: confirmed (she freezes) or called off (she is back as
  // she was). Only here, logged in; a device token cannot do either.
  const moveAction = useCallback(
    async (action: 'confirm' | 'cancel', move: HostMove) => {
      setWorking(true)
      try {
        const { host } =
          action === 'confirm'
            ? await agentService.confirmHostMove(move.id)
            : await agentService.cancelHostMove(move.id)
        setList((current) => (current ? { ...current, host } : current))
        await load()
      } catch (error) {
        fail(error, c.agentHostActionFailed)
        await load()
      } finally {
        setWorking(false)
      }
    },
    [c.agentHostActionFailed, fail, load],
  )

  const when = useCallback(
    (at: string) => new Date(at).toLocaleString(locale || undefined),
    [locale],
  )

  const devices = useMemo(() => list?.devices ?? [], [list])
  const live = devices.filter((device) => !device.revokedAt)

  const host: HostLease | null = list?.host ?? null
  const move = host?.move ?? null
  const deviceName = (id: string, name: string | null) =>
    id === 'site' ? c.agentHostSite : (name ?? c.agentHostUnknownDevice)

  // Choosing a host here comes later (H4); for now this says where she
  // lives, and a move a device asked for is confirmed or turned down here.
  const stats: ManagedListStat[] = [
    {
      key: 'live',
      label: c.agentDevicesStatLive,
      value: `${live.length} / ${list?.limit ?? 10}`,
      tone: live.length === 0 ? 'muted' : 'default',
    },
  ]

  const removedWhy = (device: SyncDevice) =>
    device.revokedReason === 'device_left'
      ? c.agentDevicesRemovedLeft
      : device.revokedReason === 'owner_changed'
        ? c.agentDevicesRemovedOwnerChanged
        : c.agentDevicesRemoved

  const items: ManagedListItem[] = devices.map((device) => {
    const badges: ManagedListItem['badges'] = []
    if (!device.revokedAt && host?.holder === device.id) {
      badges.push({ label: c.agentDevicesHostBadge, tone: 'default' })
    }
    if (
      !device.revokedAt &&
      move &&
      (move.from === device.id || move.to === device.id)
    ) {
      badges.push({
        label:
          move.phase === 'proposed'
            ? c.agentDevicesMoveAsked
            : c.agentDevicesMoving,
        tone: 'warn',
      })
    }
    if (device.revokedAt) {
      badges.push({ label: removedWhy(device), tone: 'muted' })
    } else if (device.stale && !device.connected) {
      badges.push({ label: c.agentDevicesStale, tone: 'warn' })
    }
    return {
      id: device.id,
      title: device.name,
      subtitle: `${device.tokenHint}… · ${format(c.agentDevicesAdded, { time: when(device.createdAt) })}`,
      // Connected now, or when it last was: her link records it as it opens.
      meta: device.connected
        ? c.agentDevicesConnected
        : device.lastSeenAt
          ? format(c.agentDevicesLastConnected, {
              time: when(device.lastSeenAt),
            })
          : c.agentDevicesNeverConnected,
      badges,
      actions: device.revokedAt
        ? []
        : [
            {
              key: 'remove',
              label: c.agentDevicesRemove,
              onClick: () => void remove(device),
              disabled: working,
              variant: 'danger' as const,
              confirm: format(c.agentDevicesRemoveConfirm, {
                name: device.name,
              }),
            },
          ],
    }
  })

  const form = (
    <div className="agent-devices-form">
      <InputItem
        itemKey="agent_device_name"
        label={c.agentDevicesName}
        value={name}
        onChange={setName}
        placeholder={c.agentDevicesNamePlaceholder}
        inputType="text"
        layout="vertical"
      />
      <SettingsButton
        variant="primary"
        size="sm"
        icon={<LuPlus size={14} />}
        loading={working}
        disabled={working || !name.trim()}
        onClick={() => void add()}
      >
        {c.agentDevicesCreate}
      </SettingsButton>
    </div>
  )

  return (
    <SettingGroup
      title={c.agentDevicesTitle}
      icon={<LuMonitor />}
      description={c.agentDevicesDesc}
    >
      {host ? (
        <p className="setting-hint">
          {host.state === 'vacant'
            ? c.agentHostVacant
            : format(c.agentHostLivesOn, {
                where: deviceName(host.holder, host.holderName),
                lineage: host.lineage.slice(0, 8),
                epoch: host.epoch,
              })}
        </p>
      ) : null}
      {move && move.phase === 'proposed' ? (
        <InfoActionCard
          title={format(c.agentHostProposedTitle, {
            name: deviceName(
              move.kind === 'merge' ? move.from : move.to,
              move.kind === 'merge' ? move.fromName : move.toName,
            ),
            what:
              move.kind === 'merge'
                ? c.agentHostKindMerge
                : c.agentHostKindMove,
          })}
          tone="info"
          actions={[
            {
              key: 'confirm',
              label: c.agentHostConfirm,
              onClick: () => void moveAction('confirm', move),
              disabled: working,
              variant: 'primary',
              confirm: c.agentHostConfirmConfirm,
            },
            {
              key: 'decline',
              label: c.agentHostDecline,
              onClick: () => void moveAction('cancel', move),
              disabled: working,
              variant: 'secondary',
            },
          ]}
        >
          <p className="info-action-card-lede">{c.agentHostProposedBody}</p>
        </InfoActionCard>
      ) : null}
      {move && move.phase !== 'proposed' ? (
        <InfoActionCard
          title={format(c.agentHostFrozenTitle, { step: move.step })}
          tone="warn"
          actions={[
            {
              key: 'cancel',
              label: c.agentHostCancel,
              onClick: () => void moveAction('cancel', move),
              disabled:
                working ||
                move.phase === 'committed' ||
                move.phase === 'retired',
              variant: 'secondary',
              confirm: c.agentHostCancelConfirm,
            },
          ]}
        >
          <p className="info-action-card-lede">
            {format(c.agentHostFrozenBody, {
              what:
                move.kind === 'merge'
                  ? c.agentHostKindMerge
                  : c.agentHostKindMove,
              name: deviceName(
                move.kind === 'merge' ? move.from : move.to,
                move.kind === 'merge' ? move.fromName : move.toName,
              ),
            })}
          </p>
        </InfoActionCard>
      ) : null}
      {issued ? (
        <InfoActionCard
          title={format(c.agentDevicesTokenTitle, { name: issued.name })}
          tone="info"
          fields={[
            {
              key: 'token',
              label: c.agentDevicesTokenLabel,
              value: issued.token,
              mono: true,
              copyable: true,
            },
          ]}
          actions={[
            {
              key: 'done',
              label: c.agentDevicesTokenDone,
              onClick: () => setIssued(null),
              variant: 'secondary',
            },
          ]}
        >
          <p className="info-action-card-lede">{c.agentDevicesTokenOnce}</p>
        </InfoActionCard>
      ) : null}
      <ManagedList
        stats={stats}
        formTitle={c.agentDevicesAdd}
        formDescription={c.agentDevicesAddDesc}
        formIcon={<LuPlus aria-hidden />}
        formCollapseLabel={t.common.cancel}
        formOpen={formOpen}
        onFormOpenChange={setFormOpen}
        form={form}
        items={items}
        emptyText={loading ? t.common.loading : c.agentDevicesEmpty}
        loading={loading && list === null}
        working={working}
        maxHeight={null}
      />
    </SettingGroup>
  )
}

export default AgentDevicesGroup
