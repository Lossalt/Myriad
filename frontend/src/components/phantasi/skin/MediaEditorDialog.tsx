import type { PointerEvent, ReactNode } from 'react'
import type { MediaAsset } from '../../../services/mediaApi'
import {
  LuCheck,
  LuChevronLeft,
  LuChevronRight,
  LuImage,
  LuSparkles,
  LuX,
  LuZoomIn,
  LuZoomOut,
} from '@lib/icons'
import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../../../contexts/I18nContext'
import { ApiError } from '../../../services/api'
import {
  mediaAssetSrc,
  previewMediaEdit,
  saveMediaEdit,
} from '../../../services/mediaApi'
import {
  InputItem,
  NumberItem,
  SettingsButton,
  SettingTitleTag,
  SwitchItem,
} from '../../settings'
import { displayImageUrl } from '../notes/noteImageUrl'
import {
  resizedDimensions,
  resizeMediaImage,
  validateMediaEditData,
  validMediaDimensions,
} from './mediaEdit'
import '../../ConfigForm.css'
import './MediaEditorDialog.css'

function EditorCanvas({
  zoom,
  pannable = true,
  children,
}: {
  zoom: number
  pannable?: boolean
  children: ReactNode
}) {
  const ref = useRef<HTMLDivElement>(null)
  const drag = useRef<{
    pointer: number
    x: number
    y: number
    sl: number
    st: number
  } | null>(null)
  const [panning, setPanning] = useState(false)

  useEffect(() => {
    const canvas = ref.current
    if (!canvas || zoom > 1) return
    canvas.scrollTo(0, 0)
  }, [zoom])

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (!pannable || event.button !== 0) return
    const canvas = ref.current
    if (!canvas) return
    if (
      canvas.scrollWidth <= canvas.clientWidth &&
      canvas.scrollHeight <= canvas.clientHeight
    ) {
      return
    }
    event.preventDefault()
    canvas.setPointerCapture(event.pointerId)
    drag.current = {
      pointer: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      sl: canvas.scrollLeft,
      st: canvas.scrollTop,
    }
    setPanning(true)
  }

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const start = drag.current
    const canvas = ref.current
    if (!start || !canvas) return
    canvas.scrollLeft = start.sl - (event.clientX - start.x)
    canvas.scrollTop = start.st - (event.clientY - start.y)
  }

  const endDrag = (event: PointerEvent<HTMLDivElement>) => {
    if (!drag.current || drag.current.pointer !== event.pointerId) return
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }
    drag.current = null
    setPanning(false)
  }

  return (
    <div
      ref={ref}
      className={`media-editor__canvas${pannable ? ' is-pannable' : ''}${panning ? ' is-panning' : ''}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
    >
      {children}
    </div>
  )
}

export function MediaEditorDialog({
  item,
  onClose,
  onPrevious,
  onNext,
  onSaved,
}: {
  item: MediaAsset
  onClose: () => void
  onPrevious?: () => void
  onNext?: () => void
  onSaved: (item: MediaAsset) => void
}) {
  const { t } = useI18n()
  const c = t.phantasi
  const dialog = useRef<HTMLDialogElement>(null)
  const request = useRef<AbortController | null>(null)
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const closingRef = useRef(false)
  const [closing, setClosing] = useState(false)
  const alive = useRef(true)
  const [source, setSource] = useState({ width: 0, height: 0 })
  const [dimensions, setDimensions] = useState({ width: 0, height: 0 })
  const [locked, setLocked] = useState(true)
  const [prompt, setPrompt] = useState('')
  const [draft, setDraft] = useState<{
    image: string
    generated: boolean
  } | null>(null)
  const [busy, setBusy] = useState<'generate' | 'resize' | 'save' | null>(null)
  const pending = useRef(false)
  const [error, setError] = useState('')
  const [saved, setSaved] = useState(false)
  const [zoom, setZoom] = useState(1)
  const video = item.mime.startsWith('video/')
  const editable = ['image/png', 'image/jpeg', 'image/webp'].includes(item.mime)
  const src = displayImageUrl(mediaAssetSrc(item))
  useEffect(() => {
    alive.current = true
    const opener = document.activeElement
    const modal = dialog.current
    modal?.showModal()
    const overflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      alive.current = false
      if (closeTimer.current) clearTimeout(closeTimer.current)
      request.current?.abort()
      modal?.close()
      document.body.style.overflow = overflow
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus()
    }
  }, [])
  const finishClose = () => {
    if (closeTimer.current) clearTimeout(closeTimer.current)
    onClose()
  }
  const requestClose = () => {
    if (busy === 'save' || closingRef.current) return
    closingRef.current = true
    request.current?.abort()
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      onClose()
      return
    }
    setClosing(true)
    // Fallback if another stylesheet disables animation or the event is lost.
    closeTimer.current = setTimeout(finishClose, 220)
  }
  const changeSize = (axis: 'width' | 'height', value: number) => {
    setDimensions(
      locked && source.width
        ? resizedDimensions(axis, value, source)
        : { ...dimensions, [axis]: value },
    )
  }
  const run = async (kind: 'generate' | 'resize' | 'save') => {
    if (pending.current || closingRef.current || (kind === 'resize' && !src)) return
    pending.current = true
    setBusy(kind)
    setError('')
    setSaved(false)
    const controller = new AbortController()
    request.current = controller
    try {
      if (kind === 'save' && draft) {
        const asset = await saveMediaEdit(item.id, draft.image, draft.generated)
        onSaved(asset)
        if (alive.current) {
          setDraft(null)
          setSaved(true)
        }
      } else {
        const image =
          kind === 'generate'
            ? await previewMediaEdit(
                item.id,
                prompt.trim(),
                dimensions.width,
                dimensions.height,
                controller.signal,
              )
            : await resizeMediaImage(src!, dimensions.width, dimensions.height)
        validateMediaEditData(image)
        if (alive.current && !controller.signal.aborted) {
          setDraft({ image, generated: kind === 'generate' })
          setZoom(1)
        }
      }
    } catch (err) {
      if (alive.current && !controller.signal.aborted) {
        setError(
          (err instanceof ApiError && err.code === 'MEDIA_EDIT_TOO_LARGE') ||
            (err instanceof Error && err.message === 'MEDIA_EDIT_TOO_LARGE')
            ? c.mediaEditTooLarge
            : kind === 'save'
              ? c.mediaEditSaveFailed
              : c.mediaEditFailed,
        )
      }
    } finally {
      pending.current = false
      if (alive.current) setBusy(null)
    }
  }
  const canGenerate =
    validMediaDimensions(dimensions.width, dimensions.height) &&
    dimensions.width >= 256 &&
    dimensions.height >= 256 &&
    dimensions.width <= 2048 &&
    dimensions.height <= 2048
  const feedback = error ? (
    <SettingTitleTag variant="danger">{error}</SettingTitleTag>
  ) : busy ? (
    <p role="status" className="setting-hint">
      {busy === 'save' ? c.saving : c.mediaEditWorking}
    </p>
  ) : saved ? (
    <SettingTitleTag icon={<LuCheck />}>{c.mediaEditSaved}</SettingTitleTag>
  ) : null
  return createPortal(
    <dialog
      ref={dialog}
      className={`media-editor${closing ? ' is-closing' : ''}`}
      onAnimationEnd={(event) => {
        if (
          event.target === event.currentTarget &&
          event.animationName === 'media-editor-out'
        ) {
          finishClose()
        }
      }}
      aria-label={item.name}
      onCancel={(event) => {
        event.preventDefault()
        requestClose()
      }}
    >
      <header className="media-editor__header">
        <div className="media-editor__identity">
          <span className="media-editor__file-icon">
            <LuImage aria-hidden="true" />
          </span>
          <div>
            <strong>{item.name}</strong>
            <small>
              {source.width > 0
                ? `${source.width} × ${source.height}`
                : item.mime}
            </small>
          </div>
        </div>
        <button
          type="button"
          className="modal-close-button media-editor__close"
          aria-label={c.close}
          title={c.close}
          disabled={busy === 'save'}
          onClick={requestClose}
        >
          <LuX aria-hidden="true" />
        </button>
      </header>
      <div className="media-editor__body">
        <div className="media-editor__visual">
          <div
            className={`media-editor__comparison${draft ? ' has-draft' : ''}`}
          >
            <figure>
              <figcaption className="media-editor__tag">
                {c.mediaEditOriginal}
              </figcaption>
              <EditorCanvas zoom={video ? 1 : zoom} pannable={!video}>
                {video ? (
                  <video src={src} controls />
                ) : (
                  <img
                    src={src}
                    alt={item.name}
                    draggable={false}
                    style={{
                      width: `${zoom * 100}%`,
                      height: `${zoom * 100}%`,
                      maxWidth: 'none',
                    }}
                    onError={() => setError(c.mediaEditFailed)}
                    onLoad={(event) => {
                      const image = event.currentTarget
                      const size = {
                        width: image.naturalWidth,
                        height: image.naturalHeight,
                      }
                      setSource(size)
                      setDimensions(size)
                    }}
                  />
                )}
              </EditorCanvas>
            </figure>
            {draft && (
              <figure>
                <figcaption className="media-editor__tag is-result">
                  <LuSparkles aria-hidden="true" />
                  {c.mediaEditResult}
                </figcaption>
                <EditorCanvas zoom={zoom}>
                  <img
                    src={draft.image}
                    alt={c.mediaEditResult}
                    draggable={false}
                    style={{
                      width: `${zoom * 100}%`,
                      height: `${zoom * 100}%`,
                      maxWidth: 'none',
                    }}
                  />
                </EditorCanvas>
              </figure>
            )}
          </div>
          <div className="media-editor__toolbar">
            <button
              type="button"
              disabled={!onPrevious || !!busy || !!draft}
              onClick={onPrevious}
              aria-label={c.mediaEditPrevious}
              title={c.mediaEditPrevious}
            >
              <LuChevronLeft aria-hidden="true" />
            </button>
            {!video && (
              <>
                <span className="media-editor__separator" />
                <button
                  type="button"
                  disabled={zoom <= 0.25}
                  onClick={() => setZoom((z) => Math.max(0.25, z - 0.25))}
                  aria-label={c.lightboxZoomOut}
                  title={c.lightboxZoomOut}
                >
                  <LuZoomOut aria-hidden="true" />
                </button>
                <button
                  type="button"
                  className="media-editor__zoom"
                  onClick={() => setZoom(1)}
                >
                  {Math.round(zoom * 100)}%
                </button>
                <button
                  type="button"
                  disabled={zoom >= 4}
                  onClick={() => setZoom((z) => Math.min(4, z + 0.25))}
                  aria-label={c.lightboxZoomIn}
                  title={c.lightboxZoomIn}
                >
                  <LuZoomIn aria-hidden="true" />
                </button>
                <span className="media-editor__separator" />
              </>
            )}
            <button
              type="button"
              disabled={!onNext || !!busy || !!draft}
              onClick={onNext}
              aria-label={c.mediaEditNext}
              title={c.mediaEditNext}
            >
              <LuChevronRight aria-hidden="true" />
            </button>
          </div>
          {!editable && error ? (
            <div className="media-editor__feedback">{feedback}</div>
          ) : null}
        </div>
        {editable && (
          <aside className="media-editor__controls">
            <section className="media-editor__section">
              <h3 className="media-editor__section-title">
                {c.mediaEditResolution}
              </h3>
              <div className="media-editor__dims">
                <NumberItem
                  itemKey="media-edit-width"
                  size="sm"
                  layout="vertical"
                  label={c.mediaEditWidth}
                  value={dimensions.width}
                  min={1}
                  max={8192}
                  unit="px"
                  onChange={(value) => changeSize('width', value)}
                  disabled={!!busy}
                />
                <NumberItem
                  itemKey="media-edit-height"
                  size="sm"
                  layout="vertical"
                  label={c.mediaEditHeight}
                  value={dimensions.height}
                  min={1}
                  max={8192}
                  unit="px"
                  onChange={(value) => changeSize('height', value)}
                  disabled={!!busy}
                />
              </div>
              <div className="media-editor__toggles">
                <SwitchItem
                  itemKey="media-edit-lock"
                  size="sm"
                  label={c.mediaEditLock}
                  value={locked}
                  onChange={setLocked}
                  disabled={!!busy}
                />
              </div>
              <SettingsButton
                variant="secondary"
                size="sm"
                block
                disabled={
                  !!busy ||
                  !validMediaDimensions(dimensions.width, dimensions.height) ||
                  !source.width || !src
                }
                onClick={() => void run('resize')}
              >
                {c.mediaEditResize}
              </SettingsButton>
            </section>
            <section className="media-editor__section">
              <h3 className="media-editor__section-title">{c.mediaEditAi}</h3>
              <p className="media-editor__section-hint">{c.mediaEditAiSizeHint}</p>
              <InputItem
                itemKey="media-edit-prompt"
                size="sm"
                label={c.mediaEditPrompt}
                multiline
                rows={4}
                value={prompt}
                onChange={setPrompt}
                disabled={!!busy}
              />
              <SettingsButton
                variant="primary"
                size="sm"
                block
                disabled={
                  !!busy || !prompt.trim() || !canGenerate || !source.width
                }
                icon={<LuSparkles />}
                onClick={() => void run('generate')}
              >
                {c.mediaEditGenerate}
              </SettingsButton>
            </section>
            {feedback ? (
              <div className="media-editor__feedback">{feedback}</div>
            ) : null}
            {draft ? (
              <div className="media-editor__actions">
                <SettingsButton
                  variant="secondary"
                  size="sm"
                  disabled={!!busy}
                  onClick={() => {
                    setDraft(null)
                    setError('')
                  }}
                >
                  {c.mediaEditDiscard}
                </SettingsButton>
                <SettingsButton
                  variant="primary"
                  size="sm"
                  disabled={!!busy}
                  icon={<LuCheck />}
                  onClick={() => void run('save')}
                >
                  {c.mediaEditSave}
                </SettingsButton>
              </div>
            ) : null}
          </aside>
        )}
      </div>
    </dialog>,
    document.body,
  )
}
