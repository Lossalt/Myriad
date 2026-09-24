import type { MessageBodyRef } from './messageBody'
import { useEffect, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { authSubject } from '../../utils/authSubject'
import { BODY_PAGE_CHARS, readBodyPage } from './messageBody'

/** Full text is deliberately plain text: a page never triggers whole-body Markdown parsing. */
export function AgentMessageBody({ body, preview }: { body: MessageBodyRef; preview: string }) {
  const { t } = useI18n()
  const [selection, setSelection] = useState({ id: body.id, page: 0 })
  const page = selection.id === body.id ? selection.page : 0
  const [loaded, setLoaded] = useState<{ id: string; page: number; text: string } | null>(null)
  const [error, setError] = useState(false)
  const pages = Math.ceil(body.chars / BODY_PAGE_CHARS)
  const pageChars = Math.min(BODY_PAGE_CHARS, body.chars - page * BODY_PAGE_CHARS)
  useEffect(() => {
    const controller = new AbortController()
    const signal = AbortSignal.any([controller.signal, authSubject.signal])
    setError(false)
    void readBodyPage(body, page, signal).then(text => {
      if (!signal.aborted) setLoaded({ id: body.id, page, text })
    }).catch(() => { if (!signal.aborted) setError(true) })
    return () => controller.abort()
  }, [body.id, pageChars, page])
  const text = loaded?.id === body.id && loaded.page === page ? loaded.text : page === 0 ? preview : ''
  return <div data-agent-body-page={page}>
    <div className="agent-panel-tag-actions">
      <button type="button" className="agent-panel-tag" disabled={page === 0} aria-label={t.common.back} onClick={() => setSelection({ id: body.id, page: page - 1 })}>←</button>
      <span aria-live="polite">{page + 1} / {pages}</span>
      <button type="button" className="agent-panel-tag" disabled={page + 1 >= pages} aria-label={t.common.go} onClick={() => setSelection({ id: body.id, page: page + 1 })}>→</button>
    </div>
    {error ? <p role="alert">{t.agentPanel.sessions.loadFailed}</p> : <p className="agent-md-p" style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>{text || t.common.loading}</p>}
  </div>
}
