import type {
  HostImport,
  HostLease,
  HostMove,
  SyncDevice,
  SyncDeviceList,
} from '../../services/agent/agentApi'
import type { ManagedListItem, ManagedListStat } from '../settings/ManagedList'
import { LuDownload, LuMonitor, LuPlus, LuUpload } from '@lib/icons'

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
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
  const [exportOpen, setExportOpen] = useState(false)
  const [passphrase, setPassphrase] = useState('')
  const [again, setAgain] = useState('')
  const [importOpen, setImportOpen] = useState(false)
  const [importFile, setImportFile] = useState<File | null>(null)
  const [importPassphrase, setImportPassphrase] = useState('')
  const [importProgress, setImportProgress] = useState<string | null>(null)
  const [landedNote, setLandedNote] = useState<string | null>(null)
  const fileInput = useRef<HTMLInputElement>(null)

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

  // 「导出她」: always sealed with the passphrase typed here, used once by
  // the site for the key and kept nowhere (H3-6, H3b-3).
  const exportHer = useCallback(async () => {
    if (!passphrase || passphrase !== again) return
    setWorking(true)
    try {
      const blob = await agentService.exportHer(passphrase)
      const stamp = new Date()
        .toISOString()
        .slice(0, 16)
        .replace(/[-:]/g, '')
        .replace('T', '-')
      const url = URL.createObjectURL(blob)
      const anchor = document.createElement('a')
      anchor.href = url
      anchor.download = `her-${stamp}.her-archive`
      document.body.appendChild(anchor)
      anchor.click()
      document.body.removeChild(anchor)
      URL.revokeObjectURL(url)
      setPassphrase('')
      setAgain('')
      setExportOpen(false)
    } catch (error) {
      fail(error, c.agentHostExportFailed)
    } finally {
      setWorking(false)
    }
  }, [again, c.agentHostExportFailed, fail, passphrase])

  // Her coming into this site, which has none (H3c): a device's request is
  // confirmed or turned down here, logged in; one under way can be called
  // off, and the site is as it was.
  const importAction = useCallback(
    async (action: 'confirm' | 'cancel', pending: HostImport) => {
      setWorking(true)
      try {
        setList(
          action === 'confirm'
            ? await agentService.confirmHostImport(pending.id)
            : await agentService.cancelHostImport(pending.id),
        )
      } catch (error) {
        fail(error, c.agentHostImportFailed)
        await load()
      } finally {
        setWorking(false)
      }
    },
    [c.agentHostImportFailed, fail, load],
  )

  const countsLine = useCallback(
    (counts: Record<string, number>) =>
      format(c.agentHostImportCounts, {
        n: Object.values(counts).reduce((sum, n) => sum + n, 0),
        memory: counts.memory ?? 0,
        diary: counts.diary ?? 0,
        conversation: counts.conversation ?? 0,
        outfit: counts.outfit ?? 0,
      }),
    [c.agentHostImportCounts, format],
  )

  // From an export file: sent in pieces (an interrupted upload of the same
  // file goes on where it stopped), then opened here once with the
  // passphrase, which the site uses for the key and keeps nowhere.
  const importFromFile = useCallback(async () => {
    const file = importFile
    if (!file || !importPassphrase) return
    setWorking(true)
    setLandedNote(null)
    const mb = (bytes: number) => `${(bytes / 1048576).toFixed(1)} MB`
    try {
      const pending = list?.import
      let session: HostImport
      let piece = 8 * 1024 * 1024
      if (
        pending &&
        pending.source === 'file' &&
        pending.phase === 'uploading' &&
        pending.total === file.size
      ) {
        session = pending
      } else {
        const begun = await agentService.beginFileImport(file.size)
        session = begun.import
        piece = begun.pieceBytes
      }
      let offset = session.received
      while (offset < file.size) {
        setImportProgress(
          format(c.agentHostImportFileUploading, {
            done: mb(offset),
            total: mb(file.size),
          }),
        )
        const sent = await agentService.sendFileImportPiece(
          session.id,
          offset,
          file.slice(offset, Math.min(file.size, offset + piece)),
        )
        offset = sent.import.received
      }
      setImportProgress(c.agentHostImportFileOpening)
      const landed = await agentService.openFileImport(
        session.id,
        importPassphrase,
      )
      setImportPassphrase('')
      setImportFile(null)
      setImportOpen(false)
      setLandedNote(
        format(c.agentHostImportDone, { counts: countsLine(landed.counts) }) +
          (landed.herOn ? '' : ` ${c.agentHostImportHerOff}`),
      )
    } catch (error) {
      fail(error, c.agentHostImportFailed)
    } finally {
      setImportProgress(null)
      setWorking(false)
      await load()
    }
  }, [
    c,
    countsLine,
    fail,
    format,
    importFile,
    importPassphrase,
    list?.import,
    load,
  ])

  const clearHeld = useCallback(async () => {
    setWorking(true)
    try {
      await agentService.clearHeld()
      await load()
    } catch (error) {
      fail(error, c.agentHostHeldClearFailed)
    } finally {
      setWorking(false)
    }
  }, [c.agentHostHeldClearFailed, fail, load])

  const when = useCallback(
    (at: string) => new Date(at).toLocaleString(locale || undefined),
    [locale],
  )

  const devices = useMemo(() => list?.devices ?? [], [list])
  const live = devices.filter((device) => !device.revokedAt)

  const host: HostLease | null = list?.host ?? null
  const pending: HostImport | null = list?.import ?? null
  // A move that is an import has its own card below.
  const move = host?.move && !host.move.id.startsWith('imp_') ? host.move : null
  const deviceName = (id: string, name: string | null) =>
    id === 'site' ? c.agentHostSite : (name ?? c.agentHostUnknownDevice)
  // Her copy here is her only while she lives here or is moving (H3b-5).
  const exportable =
    host !== null &&
    (host.state === 'frozen' ||
      (host.state === 'live' && host.holder === 'site'))
  const livesElsewhere =
    host !== null && host.state === 'live' && host.holder !== 'site'

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
      {pending && pending.source === 'device' ? (
        pending.phase === 'proposed' ? (
          <InfoActionCard
            title={format(c.agentHostImportProposedTitle, {
              device: pending.deviceName ?? c.agentHostUnknownDevice,
              name: pending.name ?? '—',
            })}
            tone="info"
            actions={[
              {
                key: 'confirm',
                label: c.agentHostImportConfirm,
                onClick: () => void importAction('confirm', pending),
                disabled: working,
                variant: 'primary',
                confirm: c.agentHostImportConfirmConfirm,
              },
              {
                key: 'decline',
                label: c.agentHostImportDecline,
                onClick: () => void importAction('cancel', pending),
                disabled: working,
                variant: 'secondary',
              },
            ]}
          >
            <p className="info-action-card-lede">
              {c.agentHostImportProposedBody}
            </p>
            <p className="setting-hint">{countsLine(pending.counts)}</p>
          </InfoActionCard>
        ) : (
          <InfoActionCard
            title={format(c.agentHostImportMovingTitle, {
              device: pending.deviceName ?? c.agentHostUnknownDevice,
            })}
            tone="warn"
            actions={[
              {
                key: 'cancel',
                label: c.agentHostImportCancel,
                onClick: () => void importAction('cancel', pending),
                disabled: working,
                variant: 'secondary',
                confirm: c.agentHostImportCancelConfirm,
              },
            ]}
          >
            <p className="info-action-card-lede">
              {c.agentHostImportMovingBody}
            </p>
            <p className="setting-hint">{countsLine(pending.counts)}</p>
          </InfoActionCard>
        )
      ) : null}
      {landedNote ? <p className="setting-hint">{landedNote}</p> : null}
      {list?.empty && !(pending && pending.source === 'device') ? (
        importOpen ? (
          <InfoActionCard
            title={c.agentHostImportFileTitle}
            tone="info"
            actions={[
              {
                key: 'import',
                label: c.agentHostImportFileGo,
                onClick: () => void importFromFile(),
                disabled: working || !importFile || !importPassphrase,
                variant: 'primary',
              },
              {
                key: 'close',
                label: t.common.cancel,
                onClick: () => {
                  setImportOpen(false)
                  setImportFile(null)
                  setImportPassphrase('')
                },
                disabled: working,
                variant: 'secondary',
              },
            ]}
          >
            <p className="info-action-card-lede">{c.agentHostImportFileBody}</p>
            <div className="agent-devices-form">
              <SettingsButton
                variant="secondary"
                size="sm"
                icon={<LuUpload size={14} />}
                disabled={working}
                onClick={() => fileInput.current?.click()}
              >
                {importFile ? importFile.name : c.agentHostImportFilePick}
              </SettingsButton>
              <input
                ref={fileInput}
                type="file"
                accept=".her-archive"
                className="hidden"
                title={c.agentHostImportFilePick}
                onChange={(event) =>
                  setImportFile(event.target.files?.[0] ?? null)
                }
              />
            </div>
            <InputItem
              itemKey="agent_import_passphrase"
              label={c.agentHostImportFilePassphrase}
              value={importPassphrase}
              onChange={setImportPassphrase}
              inputType="password"
              autoComplete="off"
              layout="vertical"
            />
            {importProgress ? (
              <p className="setting-hint">{importProgress}</p>
            ) : null}
          </InfoActionCard>
        ) : (
          <div className="agent-devices-form">
            <SettingsButton
              variant="secondary"
              size="sm"
              icon={<LuUpload size={14} />}
              disabled={working}
              onClick={() => setImportOpen(true)}
            >
              {c.agentHostImportFile}
            </SettingsButton>
          </div>
        )
      ) : null}
      {list?.held ? (
        <InfoActionCard
          title={format(c.agentHostHeldTitle, { n: list.held })}
          tone="info"
          actions={[
            {
              key: 'clear',
              label: c.agentHostHeldClear,
              onClick: () => void clearHeld(),
              disabled: working,
              variant: 'secondary',
              confirm: c.agentHostHeldClearConfirm,
            },
          ]}
        >
          <p className="info-action-card-lede">{c.agentHostHeldBody}</p>
        </InfoActionCard>
      ) : null}
      {exportable ? (
        exportOpen ? (
          <InfoActionCard
            title={c.agentHostExportTitle}
            tone="info"
            actions={[
              {
                key: 'export',
                label: c.agentHostExportGo,
                onClick: () => void exportHer(),
                disabled: working || !passphrase || passphrase !== again,
                variant: 'primary',
              },
              {
                key: 'close',
                label: t.common.cancel,
                onClick: () => {
                  setExportOpen(false)
                  setPassphrase('')
                  setAgain('')
                },
                disabled: working,
                variant: 'secondary',
              },
            ]}
          >
            <p className="info-action-card-lede">{c.agentHostExportBody}</p>
            <InputItem
              itemKey="agent_export_passphrase"
              label={c.agentHostExportPassphrase}
              value={passphrase}
              onChange={setPassphrase}
              inputType="password"
              autoComplete="new-password"
              layout="vertical"
            />
            <InputItem
              itemKey="agent_export_passphrase_again"
              label={c.agentHostExportAgain}
              value={again}
              onChange={setAgain}
              inputType="password"
              autoComplete="new-password"
              layout="vertical"
            />
            {again && passphrase !== again ? (
              <p className="setting-hint">{c.agentHostExportMismatch}</p>
            ) : null}
          </InfoActionCard>
        ) : (
          <div className="agent-devices-form">
            <SettingsButton
              variant="secondary"
              size="sm"
              icon={<LuDownload size={14} />}
              disabled={working}
              onClick={() => setExportOpen(true)}
            >
              {c.agentHostExport}
            </SettingsButton>
          </div>
        )
      ) : livesElsewhere && host ? (
        <p className="setting-hint">
          {format(c.agentHostExportElsewhere, {
            where: deviceName(host.holder, host.holderName),
          })}
        </p>
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
