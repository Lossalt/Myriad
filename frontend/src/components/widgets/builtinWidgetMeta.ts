import type { TranslationKeys } from '../../i18n'
import type { WidgetSize } from '../widgetGridTypes'

export type BuiltinWidgetHost = 'home' | 'control-panel' | 'note'

export type BuiltinWidgetId =
  | 'welcome'
  | 'agent-persona'
  | 'quick-stats'
  | 'recent-activity'
  | 'friend-links'
  | 'weather'
  | 'quote'
  | 'music-player'
  | 'report-bilibili'
  | 'report-steam'
  | 'report-github'
  | 'report-youtube'
  | 'report-netease'
  | 'report-bangumi'
  | 'report-mal'
  | 'report-x'
  | 'report-discord'
  | 'report-xbox'
  | 'report-psn'
  | 'social-network'
  | 'tapp-shortcut'
  | 'game-presence'
  | 'visitor-stats'
  | 'github-repos'

type WidgetsI18n = TranslationKeys['widgets']

export interface BuiltinWidgetMeta {
  id: BuiltinWidgetId
  defaultSize: WidgetSize
  supportedSizes: readonly WidgetSize[]
  hosts?: readonly BuiltinWidgetHost[]
  nameKey: keyof WidgetsI18n
}

/** 小组件目录的尺寸和宿主。不含组件实现，也不经过路由拆包。 */
export const BUILTIN_WIDGET_META: readonly BuiltinWidgetMeta[] = [
  { id: 'welcome', defaultSize: '4x2', supportedSizes: ['2x2', '4x2'], hosts: ['home', 'control-panel'], nameKey: 'welcome' },
  { id: 'agent-persona', defaultSize: '2x2', supportedSizes: ['2x2', '4x4'], hosts: ['home'], nameKey: 'agentPersona' },
  { id: 'quick-stats', defaultSize: '4x2', supportedSizes: ['4x2'], nameKey: 'quickStats' },
  { id: 'recent-activity', defaultSize: '4x2', supportedSizes: ['2x2', '4x2', '4x4'], nameKey: 'recentActivity' },
  { id: 'friend-links', defaultSize: '4x2', supportedSizes: ['4x1', '2x2', '4x2'], hosts: ['home', 'note'], nameKey: 'friendLinks' },
  { id: 'weather', defaultSize: '2x2', supportedSizes: ['2x2', '4x2', '4x1'], nameKey: 'weather' },
  { id: 'quote', defaultSize: '2x2', supportedSizes: ['2x2', '4x2', '4x1'], nameKey: 'quote' },
  { id: 'music-player', defaultSize: '2x2', supportedSizes: ['2x2', '4x2'], nameKey: 'musicPlayer' },
  { id: 'report-bilibili', defaultSize: '4x2', supportedSizes: ['4x2'], nameKey: 'reportBilibili' },
  { id: 'report-steam', defaultSize: '4x2', supportedSizes: ['4x2'], nameKey: 'reportSteam' },
  { id: 'report-github', defaultSize: '4x2', supportedSizes: ['4x2'], nameKey: 'reportGithub' },
  { id: 'report-youtube', defaultSize: '4x2', supportedSizes: ['4x2'], nameKey: 'reportYoutube' },
  { id: 'report-netease', defaultSize: '4x2', supportedSizes: ['4x2'], nameKey: 'reportNetease' },
  { id: 'report-bangumi', defaultSize: '4x2', supportedSizes: ['4x2'], nameKey: 'reportBangumi' },
  { id: 'report-mal', defaultSize: '4x2', supportedSizes: ['4x2'], nameKey: 'reportMal' },
  { id: 'report-x', defaultSize: '4x2', supportedSizes: ['4x2'], nameKey: 'reportX' },
  { id: 'report-discord', defaultSize: '4x2', supportedSizes: ['4x2'], nameKey: 'reportDiscord' },
  { id: 'report-xbox', defaultSize: '4x2', supportedSizes: ['4x2'], nameKey: 'reportXbox' },
  { id: 'report-psn', defaultSize: '4x2', supportedSizes: ['4x2'], nameKey: 'reportPsn' },
  { id: 'social-network', defaultSize: '1x1', supportedSizes: ['1x1', '2x1', '2x2'], nameKey: 'socialNetwork' },
  { id: 'tapp-shortcut', defaultSize: '1x1', supportedSizes: ['1x1', '2x1', '2x2'], nameKey: 'tappShortcut' },
  { id: 'game-presence', defaultSize: '4x2', supportedSizes: ['4x2'], nameKey: 'gamePresence' },
  { id: 'visitor-stats', defaultSize: '4x2', supportedSizes: ['2x2', '4x2'], nameKey: 'visitorStats' },
  { id: 'github-repos', defaultSize: '2x2', supportedSizes: ['2x1', '2x2', '4x2'], nameKey: 'githubRepos' },
]

export function builtinWidgetVisibleOn(
  meta: BuiltinWidgetMeta,
  host: BuiltinWidgetHost,
): boolean {
  return !meta.hosts || meta.hosts.includes(host)
}
