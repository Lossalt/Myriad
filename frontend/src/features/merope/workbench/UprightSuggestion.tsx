import type { UprightCheck, UprightReason } from '../uprightApi'
import { useEffect, useRef, useState } from 'react'
import { SettingsButton } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { userFacingError } from '../../../utils/userFacingError'
import { drawUprightPortrait, readPortraitFraming } from '../uprightApi'
import { settleUprightCheck } from './uprightRequests'
import './uprightSuggestion.css'

interface Props {
  /** The set the picture is of (`MASTER_SLOT` for the worn bust). */
  slot: string
  /** The wardrobe set, for the server; none for the worn bust. */
  outfitId?: string
  /** The picture as uploaded, shown beside its redraw. */
  originalUrl: string | null
  /** Puts the taken redraw where the picture is, as an upload would. */
  upload: (file: File) => Promise<void>
  onBusyChange?: (busy: boolean) => void
}

type Phase =
  | { kind: 'reading' }
  | { kind: 'failed'; message: string }
  | { kind: 'suggest' }
  | { kind: 'drawing' }
  | { kind: 'review'; image: Blob; url: string; check: UprightCheck | null }
  | { kind: 'adopting'; image: Blob; url: string; check: UprightCheck | null }

/**
 * After an upload: whether the picture is square to the camera, arms down,
 * nothing held, on a plain backdrop, as the rig needs. When it is not, it
 * can be redrawn that way in its own style; the owner sees both pictures and
 * what changed, and takes the redraw or keeps the original.
 */
export function UprightSuggestion({ slot, outfitId, originalUrl, upload, onBusyChange }: Props) {
  const { t, locale } = useI18n()
  const labels = t.merope.upright
  const [phase, setPhase] = useState<Phase>({ kind: 'reading' })
  const [reasons, setReasons] = useState<UprightReason[]>([])
  const [error, setError] = useState('')
  const objectUrl = useRef<string | null>(null)

  const busy = phase.kind === 'reading' || phase.kind === 'drawing' || phase.kind === 'adopting'
  useEffect(() => {
    onBusyChange?.(busy)
  }, [busy, onBusyChange])
  useEffect(() => () => onBusyChange?.(false), [onBusyChange])

  useEffect(() => {
    let live = true
    void readPortraitFraming(outfitId)
      .then((found) => {
        if (!live) return
        if (!found.length) {
          settleUprightCheck(slot)
          return
        }
        setReasons(found)
        setPhase({ kind: 'suggest' })
      })
      .catch((reason) => {
        if (live) setPhase({ kind: 'failed', message: userFacingError(reason, labels.readFailed) })
      })
    return () => {
      live = false
    }
  }, [slot, outfitId, labels.readFailed])

  useEffect(
    () => () => {
      if (objectUrl.current) URL.revokeObjectURL(objectUrl.current)
    },
    [],
  )

  const keep = () => settleUprightCheck(slot)

  const redraw = async () => {
    setError('')
    setPhase({ kind: 'drawing' })
    try {
      const { image, check } = await drawUprightPortrait(outfitId, locale)
      if (objectUrl.current) URL.revokeObjectURL(objectUrl.current)
      objectUrl.current = URL.createObjectURL(image)
      setPhase({ kind: 'review', image, url: objectUrl.current, check })
    } catch (reason) {
      setError(userFacingError(reason, t.merope.uprightFailed))
      setPhase({ kind: 'suggest' })
    }
  }

  const take = async () => {
    if (phase.kind !== 'review') return
    setError('')
    setPhase({ ...phase, kind: 'adopting' })
    try {
      const extension = phase.image.type === 'image/jpeg' ? 'jpg' : phase.image.type === 'image/webp' ? 'webp' : 'png'
      await upload(new File([phase.image], `portrait-upright.${extension}`, { type: phase.image.type || 'image/png' }))
      settleUprightCheck(slot)
    } catch (reason) {
      setError(userFacingError(reason, labels.useFailed))
      setPhase({ ...phase, kind: 'review' })
    }
  }

  if (phase.kind === 'reading') {
    return <p className="merope-upright__status">{labels.reading}</p>
  }
  if (phase.kind === 'failed') {
    return (
      <section className="merope-upright" aria-live="polite">
        <p className="merope-upright__note">{phase.message}</p>
        <div className="merope-upright__actions">
          <SettingsButton type="button" size="sm" onClick={keep}>
            {labels.dismiss}
          </SettingsButton>
        </div>
      </section>
    )
  }

  const shown = phase.kind === 'review' || phase.kind === 'adopting' ? phase : null
  const check = shown?.check ?? null
  return (
    <section className="merope-upright" aria-live="polite">
      <p className="merope-upright__title">{labels.title}</p>
      <ul className="merope-upright__reasons">
        {reasons.map((reason) => (
          <li key={reason}>{labels.reason[reason]}</li>
        ))}
      </ul>
      {shown ? null : <p className="merope-upright__note">{labels.lead}</p>}
      {shown ? (
        <>
          <div className="merope-upright__pair">
            <figure>
              {originalUrl ? <img src={originalUrl} alt="" draggable={false} /> : null}
              <figcaption>{labels.original}</figcaption>
            </figure>
            <figure>
              <img src={shown.url} alt="" draggable={false} />
              <figcaption>{labels.redrawn}</figcaption>
            </figure>
          </div>
          {check ? (
            <ul className="merope-upright__check">
              <li data-ok={check.sameCharacter}>
                {check.sameCharacter ? labels.sameCharacter : labels.notSameCharacter}
              </li>
              <li data-ok={check.sameStyle}>
                {check.sameStyle ? labels.sameStyle : labels.notSameStyle}
              </li>
              {check.upright ? null : <li data-ok={false}>{labels.notUpright}</li>}
              {check.differences.length ? (
                <li>
                  {labels.changes}
                  {check.differences.join(labels.separator)}
                </li>
              ) : null}
            </ul>
          ) : (
            <p className="merope-upright__note">{labels.unchecked}</p>
          )}
        </>
      ) : null}
      {phase.kind === 'drawing' ? (
        <p className="merope-upright__note">{labels.drawing}</p>
      ) : null}
      {error ? <p className="merope-upright__note merope-upright__error">{error}</p> : null}
      <div className="merope-upright__actions">
        {shown ? (
          <>
            <SettingsButton
              type="button"
              size="sm"
              variant="primary"
              loading={phase.kind === 'adopting'}
              disabled={phase.kind === 'adopting'}
              onClick={() => void take()}
            >
              {phase.kind === 'adopting' ? labels.using : labels.use}
            </SettingsButton>
            <SettingsButton
              type="button"
              size="sm"
              disabled={phase.kind === 'adopting'}
              onClick={() => void redraw()}
            >
              {labels.again}
            </SettingsButton>
          </>
        ) : (
          <SettingsButton
            type="button"
            size="sm"
            variant="primary"
            loading={phase.kind === 'drawing'}
            disabled={phase.kind === 'drawing'}
            onClick={() => void redraw()}
          >
            {phase.kind === 'drawing' ? labels.drawingShort : labels.redraw}
          </SettingsButton>
        )}
        <SettingsButton
          type="button"
          size="sm"
          variant="ghost"
          disabled={phase.kind === 'drawing' || phase.kind === 'adopting'}
          onClick={keep}
        >
          {labels.keep}
        </SettingsButton>
      </div>
    </section>
  )
}
