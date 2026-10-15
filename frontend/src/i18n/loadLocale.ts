import type {
  LocaleAgentCaps,
  LocaleConfig,
  LocaleEmbed,
  LocaleMerope,
  LocalePhantasi,
  LocaleStudio,
  LocaleTapp,
  ShellChromeKeys,
  ShellNamespace,
  ShellTranslationKeys,
  TranslationKeys,
} from './assembleLocale'
import type { Locale } from './locales'
import { createLocaleLoader } from './createLocaleLoader'

// Vite serves `*.json?import` as JS. A JSON import attribute makes the
// browser require application/json and reject the module.

async function loadShellPack(locale: Locale): Promise<ShellChromeKeys> {
  switch (locale) {
    case 'zh-CN': {
      const [core, config, errors] = await Promise.all([
        import('./zh-CN.json'),
        import('./configService.zh-CN.json'),
        import('./errors.zh-CN.json'),
      ])
      return { ...core.default, config: config.default, errors: errors.default }
    }
    case 'en-US': {
      const [core, config, errors] = await Promise.all([
        import('./en-US.json'),
        import('./configService.en-US.json'),
        import('./errors.en-US.json'),
      ])
      return { ...core.default, config: config.default, errors: errors.default }
    }
    case 'ja-JP': {
      const [core, config, errors] = await Promise.all([
        import('./ja-JP.json'),
        import('./configService.ja-JP.json'),
        import('./errors.ja-JP.json'),
      ])
      return { ...core.default, config: config.default, errors: errors.default }
    }
    case 'zh-TW': {
      const [core, config, errors] = await Promise.all([
        import('./zh-TW.json'),
        import('./configService.zh-TW.json'),
        import('./errors.zh-TW.json'),
      ])
      return { ...core.default, config: config.default, errors: errors.default }
    }
    case 'ko-KR': {
      const [core, config, errors] = await Promise.all([
        import('./ko-KR.json'),
        import('./configService.ko-KR.json'),
        import('./errors.ko-KR.json'),
      ])
      return { ...core.default, config: config.default, errors: errors.default }
    }
    case 'fr-FR': {
      const [core, config, errors] = await Promise.all([
        import('./fr-FR.json'),
        import('./configService.fr-FR.json'),
        import('./errors.fr-FR.json'),
      ])
      return { ...core.default, config: config.default, errors: errors.default }
    }
    case 'de-DE': {
      const [core, config, errors] = await Promise.all([
        import('./de-DE.json'),
        import('./configService.de-DE.json'),
        import('./errors.de-DE.json'),
      ])
      return { ...core.default, config: config.default, errors: errors.default }
    }
    default: {
      const _exhaustive: never = locale
      throw new Error(`Unknown locale: ${_exhaustive}`)
    }
  }
}

const shellLoader = createLocaleLoader<ShellChromeKeys>({
  'zh-CN': () => loadShellPack('zh-CN'),
  'zh-TW': () => loadShellPack('zh-TW'),
  'en-US': () => loadShellPack('en-US'),
  'ja-JP': () => loadShellPack('ja-JP'),
  'ko-KR': () => loadShellPack('ko-KR'),
  'fr-FR': () => loadShellPack('fr-FR'),
  'de-DE': () => loadShellPack('de-DE'),
})

const configLoader = createLocaleLoader<LocaleConfig>({
  'zh-CN': () => import('./config.zh-CN.json').then(module => module.default),
  'zh-TW': () => import('./config.zh-TW.json').then(module => module.default),
  'en-US': () => import('./config.en-US.json').then(module => module.default),
  'ja-JP': () => import('./config.ja-JP.json').then(module => module.default),
  'ko-KR': () => import('./config.ko-KR.json').then(module => module.default),
  'fr-FR': () => import('./config.fr-FR.json').then(module => module.default),
  'de-DE': () => import('./config.de-DE.json').then(module => module.default),
})

const tappLoader = createLocaleLoader<LocaleTapp>({
  'zh-CN': () => import('./tapp.zh-CN.json').then(module => module.default),
  'zh-TW': () => import('./tapp.zh-TW.json').then(module => module.default),
  'en-US': () => import('./tapp.en-US.json').then(module => module.default),
  'ja-JP': () => import('./tapp.ja-JP.json').then(module => module.default),
  'ko-KR': () => import('./tapp.ko-KR.json').then(module => module.default),
  'fr-FR': () => import('./tapp.fr-FR.json').then(module => module.default),
  'de-DE': () => import('./tapp.de-DE.json').then(module => module.default),
})

const phantasiLoader = createLocaleLoader<LocalePhantasi>({
  'zh-CN': () => import('./phantasi.zh-CN.json').then(module => module.default),
  'zh-TW': () => import('./phantasi.zh-TW.json').then(module => module.default),
  'en-US': () => import('./phantasi.en-US.json').then(module => module.default),
  'ja-JP': () => import('./phantasi.ja-JP.json').then(module => module.default),
  'ko-KR': () => import('./phantasi.ko-KR.json').then(module => module.default),
  'fr-FR': () => import('./phantasi.fr-FR.json').then(module => module.default),
  'de-DE': () => import('./phantasi.de-DE.json').then(module => module.default),
})

const studioLoader = createLocaleLoader<LocaleStudio>({
  'zh-CN': () => import('./studio.zh-CN.json').then(module => module.default),
  'zh-TW': () => import('./studio.zh-TW.json').then(module => module.default),
  'en-US': () => import('./studio.en-US.json').then(module => module.default),
  'ja-JP': () => import('./studio.ja-JP.json').then(module => module.default),
  'ko-KR': () => import('./studio.ko-KR.json').then(module => module.default),
  'fr-FR': () => import('./studio.fr-FR.json').then(module => module.default),
  'de-DE': () => import('./studio.de-DE.json').then(module => module.default),
})

const meropeLoader = createLocaleLoader<LocaleMerope>({
  'zh-CN': () => import('./merope.zh-CN.json').then(module => module.default),
  'zh-TW': () => import('./merope.zh-TW.json').then(module => module.default),
  'en-US': () => import('./merope.en-US.json').then(module => module.default),
  'ja-JP': () => import('./merope.ja-JP.json').then(module => module.default),
  'ko-KR': () => import('./merope.ko-KR.json').then(module => module.default),
  'fr-FR': () => import('./merope.fr-FR.json').then(module => module.default),
  'de-DE': () => import('./merope.de-DE.json').then(module => module.default),
})

const embedLoader = createLocaleLoader<LocaleEmbed>({
  'zh-CN': () => import('./embed.zh-CN.json').then(module => module.default),
  'zh-TW': () => import('./embed.zh-TW.json').then(module => module.default),
  'en-US': () => import('./embed.en-US.json').then(module => module.default),
  'ja-JP': () => import('./embed.ja-JP.json').then(module => module.default),
  'ko-KR': () => import('./embed.ko-KR.json').then(module => module.default),
  'fr-FR': () => import('./embed.fr-FR.json').then(module => module.default),
  'de-DE': () => import('./embed.de-DE.json').then(module => module.default),
})

const agentCapsLoader = createLocaleLoader<LocaleAgentCaps>({
  'zh-CN': () => import('./agentCaps.zh-CN.json').then(module => module.default),
  'zh-TW': () => import('./agentCaps.zh-TW.json').then(module => module.default),
  'en-US': () => import('./agentCaps.en-US.json').then(module => module.default),
  'ja-JP': () => import('./agentCaps.ja-JP.json').then(module => module.default),
  'ko-KR': () => import('./agentCaps.ko-KR.json').then(module => module.default),
  'fr-FR': () => import('./agentCaps.fr-FR.json').then(module => module.default),
  'de-DE': () => import('./agentCaps.de-DE.json').then(module => module.default),
})

const namespaceLoaders = {
  tapp: tappLoader,
  phantasi: phantasiLoader,
  studio: studioLoader,
  embed: embedLoader,
  merope: meropeLoader,
  agentCaps: agentCapsLoader,
} as const

export class LocaleNamespaceError extends Error {
  constructor(readonly locale: Locale, cause: unknown) {
    super(`Failed to load settings translations for ${locale}`, { cause })
    this.name = 'LocaleNamespaceError'
  }
}

async function loadFullPack(locale: Locale): Promise<TranslationKeys> {
  const [shell, config, tapp, phantasi, studio, embed, merope, agentCaps] = await Promise.all([
    shellLoader.load(locale),
    configLoader.load(locale).catch((cause) => {
      throw new LocaleNamespaceError(locale, cause)
    }),
    tappLoader.load(locale),
    phantasiLoader.load(locale),
    studioLoader.load(locale),
    embedLoader.load(locale),
    meropeLoader.load(locale),
    agentCapsLoader.load(locale),
  ])
  return { ...shell, config, tapp, phantasi, studio, embed, merope, agentCaps }
}

const loader = createLocaleLoader<TranslationKeys>({
  'zh-CN': () => loadFullPack('zh-CN'),
  'zh-TW': () => loadFullPack('zh-TW'),
  'en-US': () => loadFullPack('en-US'),
  'ja-JP': () => loadFullPack('ja-JP'),
  'ko-KR': () => loadFullPack('ko-KR'),
  'fr-FR': () => loadFullPack('fr-FR'),
  'de-DE': () => loadFullPack('de-DE'),
})

/** Chrome only. Tapp / Phantasi / Merope catalogs load through I18nNamespace. */
export function loadShellLocale(locale: Locale): Promise<ShellChromeKeys> {
  return shellLoader.load(locale)
}

export function getCachedShellLocale(locale: Locale): ShellChromeKeys | null {
  return shellLoader.getCached(locale)
}

export function readShellLocale(locale: Locale): ShellChromeKeys {
  return shellLoader.read(locale)
}

export function loadShellNamespace<N extends ShellNamespace>(
  name: N,
  locale: Locale,
): Promise<ShellTranslationKeys[N]> {
  return namespaceLoaders[name].load(locale) as Promise<ShellTranslationKeys[N]>
}

export function getCachedShellNamespace<N extends ShellNamespace>(
  name: N,
  locale: Locale,
): ShellTranslationKeys[N] | null {
  return namespaceLoaders[name].getCached(locale) as ShellTranslationKeys[N] | null
}

/** Only call during React render; event handlers use loadShellNamespace. */
export function readShellNamespace<N extends ShellNamespace>(
  name: N,
  locale: Locale,
): ShellTranslationKeys[N] {
  return namespaceLoaders[name].read(locale) as ShellTranslationKeys[N]
}

/** Settings consumers call this during render, before callbacks can use their copy. */
export function readConfigLocale(locale: Locale): TranslationKeys {
  return loader.read(locale)
}

/** Explicit full-pack API retained for consumers that need settings strings. */
export function loadLocale(locale: Locale): Promise<TranslationKeys> {
  return loader.load(locale)
}

export function getCachedLocale(locale: Locale): TranslationKeys | null {
  return loader.getCached(locale)
}

export type { ShellChromeKeys, ShellNamespace }
