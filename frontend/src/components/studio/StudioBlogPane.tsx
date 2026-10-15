import type { ReactNode } from 'react'
import type { BlogSettings, BlogSnapshot, BlogTheme } from '../../types/blog'
import { useEffect, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import * as blogApi from '../../services/blogApi'
import {
  blogBuildPolls,
  blogThemeCanUninstall,
  blogThemeInstallUrl,
  blogThemeRemovalReturnsToDefault,
  parseBlogParams,
  sameBlogSettings,
} from '../../services/blogState'
import { userFacingError } from '../../utils/userFacingError'
import { WorkbenchPage } from '../phantasi/skin/PhantasiWorkbenchChrome'
import { PhantasiWorkbenchIcon } from '../phantasi/ui/PhantasiWorkbenchIcon'
import './StudioBlogPane.css'

const POLL_MS = 2000

function paramsTextOf(settings: BlogSettings): string {
  return JSON.stringify(settings.params, null, 2)
}

export function StudioBlogPane({ back }: { back: ReactNode }) {
  const { t, format } = useI18n()
  const copy = t.studio
  const [snapshot, setSnapshot] = useState<BlogSnapshot | null>(null)
  const [draft, setDraft] = useState<BlogSettings | null>(null)
  const [paramsText, setParamsText] = useState('{}')
  const [themeUrl, setThemeUrl] = useState('')
  const [pendingRemove, setPendingRemove] = useState<BlogTheme | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(true)

  const apply = (next: BlogSnapshot, replaceForm: boolean) => {
    setSnapshot(next)
    if (replaceForm) {
      setDraft(next.settings)
      setParamsText(paramsTextOf(next.settings))
    }
  }

  useEffect(() => {
    const controller = new AbortController()
    setLoading(true)
    void blogApi.getBlogSettings(controller.signal).then((next) => {
      if (!controller.signal.aborted) apply(next, true)
    }).catch((err: unknown) => {
      if (!controller.signal.aborted) setError(userFacingError(err, copy.loadFailed))
    }).finally(() => {
      if (!controller.signal.aborted) setLoading(false)
    })
    return () => controller.abort()
  }, [copy.loadFailed])

  const status = snapshot?.build.status ?? 'idle'
  useEffect(() => {
    if (!blogBuildPolls(status)) return
    const timer = window.setInterval(() => {
      void blogApi.getBlogSettings().then((next) => {
        setSnapshot(next)
      }).catch(() => {
        // 轮询失败就留着上一份状态。
      })
    }, POLL_MS)
    return () => window.clearInterval(timer)
  }, [status])

  const params = parseBlogParams(paramsText)
  const dirty = draft != null && snapshot != null && (
    !sameBlogSettings(
      { ...draft, params: params ?? draft.params },
      snapshot.settings,
    ) || paramsText !== paramsTextOf(snapshot.settings)
  )

  const save = async (next: BlogSettings) => {
    const parsed = parseBlogParams(paramsText)
    if (!parsed || !next.theme_id.trim()) {
      setError(parsed ? copy.themeRequired : copy.paramsInvalid)
      return
    }
    setBusy(true)
    setError('')
    try {
      const saved = await blogApi.putBlogSettings({
        theme_id: next.theme_id.trim(),
        site_title: next.site_title,
        description: next.description,
        params: parsed,
      })
      apply(saved, true)
    } catch (err) {
      setError(userFacingError(err, copy.saveFailed))
    } finally {
      setBusy(false)
    }
  }

  const installFile = async (file: File) => {
    setBusy(true)
    setError('')
    try {
      await blogApi.uploadBlogTheme(file)
      apply(await blogApi.getBlogSettings(), false)
    } catch (err) {
      setError(userFacingError(err, copy.themeInstallFailed))
    } finally {
      setBusy(false)
    }
  }

  const installUrl = async () => {
    const url = blogThemeInstallUrl(themeUrl)
    if (!url) {
      setError(copy.themeUrlInvalid)
      return
    }
    setBusy(true)
    setError('')
    try {
      await blogApi.installBlogThemeFromUrl(url)
      setThemeUrl('')
      apply(await blogApi.getBlogSettings(), false)
    } catch (err) {
      setError(userFacingError(err, copy.themeInstallFailed))
    } finally {
      setBusy(false)
    }
  }

  const removeTheme = async (theme: BlogTheme) => {
    setBusy(true)
    setError('')
    try {
      await blogApi.deleteBlogTheme(theme.id)
      setPendingRemove(null)
      apply(await blogApi.getBlogSettings(), true)
    } catch (err) {
      setError(userFacingError(err, copy.themeRemoveFailed))
    } finally {
      setBusy(false)
    }
  }

  const rebuild = async () => {
    setBusy(true)
    setError('')
    try {
      const build = await blogApi.rebuildBlog()
      setSnapshot((current) => current ? { ...current, build } : current)
    } catch (err) {
      setError(userFacingError(err, copy.rebuildFailed))
    } finally {
      setBusy(false)
    }
  }

  const statusLabel = snapshot ? copy[statusKey(snapshot.build.status)] : ''

  return (
    <WorkbenchPage
      title={copy.blog}
      icon={<PhantasiWorkbenchIcon kind="studio" />}
      back={back}
      action={
        <a className="studio-blog__open" href="/blog/">{copy.openBlog}</a>
      }
    >
      <div className="studio-blog">
        {loading ? <p>{copy.loading}</p> : null}
        {error ? <p className="studio-blog__error" role="alert">{error}</p> : null}
        {draft && snapshot ? (
          <>
            <label>
              {copy.siteTitle}
              <input
                value={draft.site_title}
                onChange={(event) => setDraft({ ...draft, site_title: event.target.value })}
              />
            </label>
            <label>
              {copy.siteDescription}
              <textarea
                rows={3}
                value={draft.description}
                onChange={(event) => setDraft({ ...draft, description: event.target.value })}
              />
            </label>
            <p className="studio-blog__note">{copy.languageNote}</p>
            <section>
              <h3>{copy.themes}</h3>
              <ul className="studio-blog__themes">
                {snapshot.themes.map((theme) => {
                  const current = theme.id === draft.theme_id
                  return (
                    <li key={theme.id}>
                      <span>
                        {theme.name}
                        {theme.version ? ` ${theme.version}` : ''}
                        {theme.builtin ? ` · ${copy.themeBuiltin}` : ''}
                        {current ? ` · ${copy.themeCurrent}` : ''}
                      </span>
                      {!current ? (
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => {
                            const next = { ...draft, theme_id: theme.id }
                            setDraft(next)
                            void save(next)
                          }}
                        >
                          {copy.themeUse}
                        </button>
                      ) : null}
                      {blogThemeCanUninstall(theme) ? (
                        <button type="button" disabled={busy} onClick={() => setPendingRemove(theme)}>
                          {copy.themeRemove}
                        </button>
                      ) : null}
                    </li>
                  )
                })}
              </ul>
              {pendingRemove ? (
                <div className="studio-blog__confirm" role="alertdialog">
                  <p>
                    {blogThemeRemovalReturnsToDefault(pendingRemove.id, snapshot.settings.theme_id)
                      ? copy.themeRemoveCurrent
                      : copy.themeRemoveDetail}
                  </p>
                  <button type="button" disabled={busy} onClick={() => { void removeTheme(pendingRemove) }}>
                    {copy.themeRemoveConfirm}
                  </button>
                  <button type="button" disabled={busy} onClick={() => setPendingRemove(null)}>
                    {copy.cancel}
                  </button>
                </div>
              ) : null}
              <label>
                {copy.themeUpload}
                <input
                  type="file"
                  accept=".zip,application/zip"
                  disabled={busy}
                  onChange={(event) => {
                    const file = event.target.files?.[0]
                    event.target.value = ''
                    if (file) void installFile(file)
                  }}
                />
              </label>
              <label>
                {copy.themeUrl}
                <input value={themeUrl} onChange={(event) => setThemeUrl(event.target.value)} />
              </label>
              <button type="button" disabled={busy} onClick={() => { void installUrl() }}>
                {copy.themeInstall}
              </button>
            </section>
            <label>
              {copy.params}
              <textarea
                rows={8}
                spellCheck={false}
                value={paramsText}
                aria-invalid={params == null || undefined}
                onChange={(event) => setParamsText(event.target.value)}
              />
            </label>
            {params == null ? <p className="studio-blog__error">{copy.paramsInvalid}</p> : null}
            <p>
              {copy.hugo}: {snapshot.hugo.available
                ? (snapshot.hugo.version ? format(copy.hugoVersion, { version: snapshot.hugo.version }) : copy.hugoAvailable)
                : copy.hugoMissing}
            </p>
            {!snapshot.hugo.available ? <p role="status">{copy.hugoUnavailable}</p> : null}
            <p>
              {copy.build}: {statusLabel}
              {snapshot.build.last_built_at ? ` · ${snapshot.build.last_built_at}` : ''}
            </p>
            {snapshot.build.last_error ? (
              <p className="studio-blog__error">{copy.lastError} {snapshot.build.last_error}</p>
            ) : null}
            {snapshot.build.log_tail ? (
              <pre className="studio-blog__log" aria-label={copy.buildLog}>{snapshot.build.log_tail}</pre>
            ) : null}
            <div className="studio-blog__actions">
              <button type="button" disabled={busy || !dirty || params == null} onClick={() => { void save(draft) }}>
                {busy ? copy.saving : copy.save}
              </button>
              <button type="button" disabled={busy} onClick={() => { void rebuild() }}>
                {copy.rebuild}
              </button>
            </div>
          </>
        ) : null}
      </div>
    </WorkbenchPage>
  )
}

function statusKey(status: BlogSnapshot['build']['status']):
  | 'buildIdle'
  | 'buildQueued'
  | 'buildRunning'
  | 'buildOk'
  | 'buildFailed'
  | 'buildUnavailable' {
  switch (status) {
    case 'queued':
      return 'buildQueued'
    case 'running':
      return 'buildRunning'
    case 'ok':
      return 'buildOk'
    case 'failed':
      return 'buildFailed'
    case 'unavailable':
      return 'buildUnavailable'
    default:
      return 'buildIdle'
  }
}
