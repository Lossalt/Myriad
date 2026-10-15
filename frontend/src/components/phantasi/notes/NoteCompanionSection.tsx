import type { CompanionStatus } from '../../../services/phantasiaiApi'
import { LuSparkles as Sparkles } from '@lib/icons'
import { useEffect, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import * as phantasiaiApi from '../../../services/phantasiaiApi'
import { userFacingError } from '../../../utils/userFacingError'
import { NoteButton, NoteSection } from './NoteControls'

type Kind = 'annotations' | 'podcast'

/** 已发布笔记的 AI 注释和播客。生成后博客重建，文章页才挂上伴读岛。 */
export function NoteCompanionSection({ itemId }: { itemId: number }) {
  const { t, format } = useI18n()
  const [status, setStatus] = useState<CompanionStatus | null>(null)
  const [busy, setBusy] = useState<Kind | null>(null)
  const [error, setError] = useState('')

  useEffect(() => {
    const controller = new AbortController()
    setStatus(null)
    setError('')
    phantasiaiApi.getCompanion(itemId, controller.signal).then(setStatus, (err: unknown) => {
      if (!controller.signal.aborted) setError(userFacingError(err, t.studio.companionLoadFailed))
    })
    return () => controller.abort()
  }, [itemId, t.studio.companionLoadFailed])

  const generate = async (kind: Kind) => {
    if (busy || !status) return
    setBusy(kind)
    setError('')
    try {
      const response =
        kind === 'annotations'
          ? status.annotations > 0
            ? await phantasiaiApi.regenerateAnnotations(itemId)
            : await phantasiaiApi.getAnnotations(itemId)
          : status.podcast
            ? await phantasiaiApi.regeneratePodcastScript(itemId)
            : await phantasiaiApi.getPodcastScript(itemId)
      if (!response.success) throw new Error(response.error || t.studio.companionFailed)
      setStatus(await phantasiaiApi.getCompanion(itemId))
    } catch (err) {
      setError(userFacingError(err, t.studio.companionFailed))
    } finally {
      setBusy(null)
    }
  }

  const rows: Array<{ kind: Kind; label: string; has: boolean }> = status
    ? [
        {
          kind: 'annotations',
          has: status.annotations > 0,
          label:
            status.annotations > 0
              ? format(t.studio.companionAnnotations, { count: status.annotations })
              : t.studio.companionAnnotationsNone,
        },
        {
          kind: 'podcast',
          has: status.podcast,
          label: status.podcast ? t.studio.companionPodcastReady : t.studio.companionPodcastNone,
        },
      ]
    : []

  return (
    <NoteSection title={t.studio.companionTitle} hint={error || t.studio.companionHint}>
      {rows.map((row) => (
        <div key={row.kind} className="note-compose note-companion">
          <span className="note-companion__state">{row.label}</span>
          <NoteButton
            variant="outline"
            icon={<Sparkles />}
            loading={busy === row.kind}
            disabled={busy != null}
            onClick={() => void generate(row.kind)}
          >
            {row.has ? t.studio.companionRegenerate : t.studio.companionGenerate}
          </NoteButton>
        </div>
      ))}
    </NoteSection>
  )
}
