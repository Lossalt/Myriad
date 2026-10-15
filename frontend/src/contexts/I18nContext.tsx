import type {
  Locale,
  ShellNamespace,
  ShellTranslationKeys,
  TranslationKeys,
} from '../i18n'
import React, {
  createContext,
  Suspense,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react'
import { DocumentReady, StartupFailure } from '../components/DocumentReady'
import { formatMessage, getDefaultLocale, htmlLang, saveLocale } from '../i18n'
import {
  getCachedShellLocale,
  loadLocale,
  loadShellLocale,
  LocaleNamespaceError,
  readConfigLocale,
  readShellLocale,
  readShellNamespace,
} from '../i18n/loadLocale'
import { persistLocaleToAccount } from '../i18n/localeAccount'

// Begin the request while the rest of the app initializes, instead of waiting
// for the provider's first effect. The provider handles failure and retries.
if (typeof window !== 'undefined') {
  void loadShellLocale(getDefaultLocale()).catch(() => {})
}

interface I18nContextType {
  locale: Locale
  setLocale: (locale: Locale, options?: { persist?: boolean }) => void
  t: ShellTranslationKeys
  format: (template: string, params: Record<string, string | number>) => string
}

function createI18nContext() {
  return createContext<I18nContextType | null>(null)
}

// Survive Vite HMR: a new createContext() makes useI18n read a different object
// than the still-mounted Provider (HomeStickerCropTip then throws).
const I18nContext: React.Context<I18nContextType | null> = import.meta.hot
  ? ((import.meta.hot.data.i18nContext ??=
      createI18nContext()) as React.Context<I18nContextType | null>)
  : createI18nContext()

if (import.meta.hot) {
  import.meta.hot.data.i18nContext = I18nContext
}

interface LocaleBundle {
  locale: Locale
  t: ShellTranslationKeys
}

function asShellCopy(
  chrome: ReturnType<typeof readShellLocale>,
): ShellTranslationKeys {
  return chrome as ShellTranslationKeys
}

interface NamespaceBoundaryProps {
  locale: Locale
  copy: ShellTranslationKeys
  retry: () => Promise<unknown>
  children: React.ReactNode
}

/** Catch only catalog imports; programming errors still reach the app's error handling. */
export class LocaleNamespaceBoundary extends React.Component<NamespaceBoundaryProps, {
  error: LocaleNamespaceError | null
  retrying: boolean
  locale: Locale
}> {
  state = { error: null as LocaleNamespaceError | null, retrying: false, locale: this.props.locale }

  static getDerivedStateFromProps(props: NamespaceBoundaryProps, state: { locale: Locale }) {
    return props.locale === state.locale ? null : { error: null, retrying: false, locale: props.locale }
  }

  static getDerivedStateFromError(error: unknown) {
    if (!(error instanceof LocaleNamespaceError)) throw error
    return { error, retrying: false }
  }

  retry = async () => {
    const locale = this.props.locale
    this.setState({ retrying: true })
    try {
      await this.props.retry()
      if (this.props.locale !== locale) return
      this.setState({ error: null, retrying: false })
    } catch (error) {
      if (this.props.locale !== locale) return
      this.setState(LocaleNamespaceBoundary.getDerivedStateFromError(error))
    }
  }

  render() {
    if (!this.state.error) return this.props.children
    return (
      <DocumentReady>
        <div role="alert" className="p-6 text-center">
          <p>{this.props.copy.errors.localeLoadFailed}</p>
          <button type="button" disabled={this.state.retrying} onClick={this.retry}>
            {this.state.retrying ? this.props.copy.common.loading : this.props.copy.common.retry}
          </button>
        </div>
      </DocumentReady>
    )
  }
}

export const I18nProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [bootFailed, setBootFailed] = useState(false)
  const [locale, setLocaleState] = useState<Locale>(getDefaultLocale)
  // Swap locale and t together so the UI never shows the wrong language.
  const [bundle, setBundle] = useState<LocaleBundle | null>(() => {
    const initial = getDefaultLocale()
    const cached = getCachedShellLocale(initial)
    return cached ? { locale: initial, t: asShellCopy(cached) } : null
  })

  useEffect(() => {
    let cancelled = false

    if (bundle?.locale === locale) return

    const load = async () => {
      try {
        const t = await loadShellLocale(locale)
        if (!cancelled) setBundle({ locale, t: asShellCopy(t) })
      } catch (error) {
        if (cancelled) return
        console.error('[I18n] Failed to load locale:', locale, error)
        try {
          if (locale === 'en-US') throw error
          const t = await loadShellLocale('en-US')
          if (cancelled) return
          setLocaleState('en-US')
          setBundle({ locale: 'en-US', t: asShellCopy(t) })
          void import('../utils/toastManager').then(({ showError }) => {
            if (!cancelled) showError(t.errors.localeLoadFailed)
          }).catch(() => {})
        } catch {
          if (!cancelled) setBootFailed(true)
        }
      }
    }
    void load()

    return () => {
      cancelled = true
    }
  }, [locale, bundle?.locale])

  // Set target locale first; swap copy with the bundle (no flash).
  const setLocale = useCallback(
    (newLocale: Locale, options?: { persist?: boolean }) => {
      setLocaleState(newLocale)
      saveLocale(newLocale)
      if (options?.persist !== false) {
        persistLocaleToAccount(newLocale)
      }
      void import('../utils/analyticsEvents').then(
        ({ trackProductEvent, AnalyticsEvents }) => {
          trackProductEvent(AnalyticsEvents.LOCALE_SWITCH, {
            target: newLocale,
            throttleMs: 3000,
          })
        },
      )
    },
    [],
  )

  // html lang tracks the loaded bundle, not the target locale.
  useEffect(() => {
    if (bundle) {
      document.documentElement.lang = htmlLang(bundle.locale)
    }
  }, [bundle])

  const format = useCallback(
    (template: string, params: Record<string, string | number>) => {
      return formatMessage(bundle?.locale ?? locale, template, params)
    },
    [bundle?.locale, locale],
  )

  const value = useMemo(() => {
    if (!bundle) return null
    return {
      locale: bundle.locale,
      setLocale,
      t: bundle.t,
      format,
    }
  }, [bundle, setLocale, format])

  // Do not mount children until the first bundle is ready.
  if (!value) {
    return bootFailed ? <StartupFailure /> : null
  }

  return (
    <I18nContext.Provider value={value}>
      <LocaleNamespaceBoundary locale={value.locale} copy={value.t} retry={() => loadLocale(value.locale)}>
        <Suspense fallback={null}>
          {children}
        </Suspense>
      </LocaleNamespaceBoundary>
    </I18nContext.Provider>
  )
}

function fallbackI18n(): I18nContextType {
  const locale = getDefaultLocale()
  // Detached React trees suspend until their actual locale is ready too.
  // Settings consumers opt into their additional namespace with useConfigI18n.
  const t = asShellCopy(readShellLocale(locale))
  return {
    locale,
    setLocale: (newLocale, options) => {
      saveLocale(newLocale)
      if (options?.persist !== false) {
        persistLocaleToAccount(newLocale)
      }
    },
    t,
    format: (template, params) => formatMessage(locale, template, params),
  }
}

export function useI18n(): I18nContextType {
  return useContext(I18nContext) ?? fallbackI18n()
}

/**
 * Load domain catalogs for a subtree. Chrome stays on the outer provider.
 *  fallback must stay null: Agent chrome mounts this at the document root.
 */
export function I18nNamespace({
  names,
  children,
}: {
  names: readonly ShellNamespace[]
  children: React.ReactNode
}) {
  return (
    <Suspense fallback={null}>
      <I18nNamespaceReady names={names}>{children}</I18nNamespaceReady>
    </Suspense>
  )
}

function I18nNamespaceReady({
  names,
  children,
}: {
  names: readonly ShellNamespace[]
  children: React.ReactNode
}) {
  const context = useI18n()
  const extras = {} as Pick<ShellTranslationKeys, ShellNamespace>
  for (const name of names) {
    extras[name] = readShellNamespace(name, context.locale) as never
  }
  const value = useMemo(
    () => ({
      ...context,
      t: { ...context.t, ...extras },
    }),
    [context, extras.tapp, extras.phantasi, extras.studio, extras.embed, extras.merope, extras.agentCaps],
  )
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>
}

/**
 * Off-route surfaces (home grid, control panel) must carry their own catalog.
 */
export function withI18nNamespace<P extends object>(
  names: readonly ShellNamespace[],
  Component: React.ComponentType<P>,
) {
  function Namespaced(props: P) {
    return (
      <I18nNamespace names={names}>
        <Component {...props} />
      </I18nNamespace>
    )
  }
  Namespaced.displayName = `I18nNamespace(${Component.displayName || Component.name || 'Component'})`
  return Namespaced
}

/** Opt into the settings namespace before rendering settings UI or its callbacks. */
export function useConfigI18n(): Omit<I18nContextType, 't'> & { t: TranslationKeys } {
  const context = useI18n()
  const t = readConfigLocale(context.locale)
  return useMemo(() => ({ ...context, t }), [context, t])
}

export type { Locale, ShellTranslationKeys, TranslationKeys }
