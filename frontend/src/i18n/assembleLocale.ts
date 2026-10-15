export type LocaleCore = typeof import('./en-US.json')
export type LocaleConfig = typeof import('./config.en-US.json')
export type LocaleTapp = typeof import('./tapp.en-US.json')
export type LocalePhantasi = typeof import('./phantasi.en-US.json')
export type LocaleStudio = typeof import('./studio.en-US.json')
export type LocaleEmbed = typeof import('./embed.en-US.json')
export type LocaleMerope = typeof import('./merope.en-US.json')
export type LocaleErrors = typeof import('./errors.en-US.json')
export type LocaleAgentCaps = typeof import('./agentCaps.en-US.json')

export type LocaleConfigService = typeof import('./configService.en-US.json')

/** Always-on chrome: core UI, errors, and settings-service strings. */
export type ShellChromeKeys = LocaleCore & {
  config: LocaleConfigService
  errors: LocaleErrors
}

export type ShellNamespace = 'tapp' | 'phantasi' | 'studio' | 'embed' | 'merope' | 'agentCaps'

export type ShellTranslationKeys = ShellChromeKeys & {
  tapp: LocaleTapp
  phantasi: LocalePhantasi
  studio: LocaleStudio
  embed: LocaleEmbed
  merope: LocaleMerope
  agentCaps: LocaleAgentCaps
}

export type TranslationKeys = Omit<ShellTranslationKeys, 'config'> & {
  config: LocaleConfig
}

export function assembleLocale(
  core: LocaleCore,
  namespaces: {
    config: LocaleConfig
    tapp: LocaleTapp
    phantasi: LocalePhantasi
    studio: LocaleStudio
    embed: LocaleEmbed
    merope: LocaleMerope
    errors: LocaleErrors
    agentCaps: LocaleAgentCaps
  },
): TranslationKeys {
  return {
    ...core,
    ...namespaces,
  }
}
