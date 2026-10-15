import type { ComponentType } from 'react'
import type { TranslationKeys } from '../../../i18n'
import type { WidgetComponentProps, WidgetType } from '../../widgetGridTypes'
import type { BuiltinWidgetId } from '../../widgets/builtinWidgetMeta'
import { lazy } from 'react'
import {
  BUILTIN_WIDGET_META,
  builtinWidgetVisibleOn,
} from '../../widgets/builtinWidgetMeta'
import {
  preloadReportCardsForTypes,
  ReportCardHost,
} from '../../widgets/reportCardHost'

type WidgetFace = ComponentType<WidgetComponentProps>
type WidgetLoader = () => Promise<{ default: WidgetFace }>

/** 笔记宿主用得到的实现。不进口 builtinWidgets，避免顺手带上路由表和手帐精选磁贴。 */
const NOTE_WIDGET_LOADERS: Partial<Record<BuiltinWidgetId, WidgetLoader>> = {
  'quick-stats': () =>
    import('../../widgets/QuickStatsWidget').then((mod) => ({ default: mod.QuickStatsWidget })),
  'recent-activity': () =>
    import('../../widgets/RecentActivityWidget').then((mod) => ({ default: mod.RecentActivityWidget })),
  'friend-links': () =>
    import('../../widgets/FriendLinksWidget').then((mod) => ({ default: mod.FriendLinksWidget })),
  weather: () =>
    import('../../widgets/WeatherWidget').then((mod) => ({ default: mod.WeatherWidget })),
  quote: () =>
    import('../../widgets/QuoteWidget').then((mod) => ({ default: mod.QuoteWidget })),
  'music-player': () =>
    import('../../widgets/MusicPlayerWidget').then((mod) => ({ default: mod.MusicPlayerWidget })),
  'social-network': () =>
    import('../../widgets/SocialNetworkWidget').then((mod) => ({ default: mod.SocialNetworkWidget })),
  'tapp-shortcut': () =>
    import('../../widgets/TappShortcutWidget').then((mod) => ({ default: mod.TappShortcutWidget })),
  'game-presence': () =>
    import('../../widgets/GamePresenceWidget').then((mod) => ({ default: mod.GamePresenceWidget })),
  'visitor-stats': () =>
    import('../../widgets/VisitorStatsWidget').then((mod) => ({ default: mod.VisitorStatsWidget })),
  'github-repos': () =>
    import('../../widgets/GithubReposWidget').then((mod) => ({ default: mod.GithubReposWidget })),
}

const NOTE_WIDGET_FACES: Partial<Record<BuiltinWidgetId, WidgetFace>> = {
  'quick-stats': lazy(NOTE_WIDGET_LOADERS['quick-stats']!),
  'recent-activity': lazy(NOTE_WIDGET_LOADERS['recent-activity']!),
  'friend-links': lazy(NOTE_WIDGET_LOADERS['friend-links']!),
  weather: lazy(NOTE_WIDGET_LOADERS.weather!),
  quote: lazy(NOTE_WIDGET_LOADERS.quote!),
  'music-player': lazy(NOTE_WIDGET_LOADERS['music-player']!),
  'social-network': lazy(NOTE_WIDGET_LOADERS['social-network']!),
  'tapp-shortcut': lazy(NOTE_WIDGET_LOADERS['tapp-shortcut']!),
  'game-presence': lazy(NOTE_WIDGET_LOADERS['game-presence']!),
  'visitor-stats': lazy(NOTE_WIDGET_LOADERS['visitor-stats']!),
  'github-repos': lazy(NOTE_WIDGET_LOADERS['github-repos']!),
}

function noteWidgetFace(id: BuiltinWidgetId): WidgetFace | null {
  if (id.startsWith('report-')) return ReportCardHost
  return NOTE_WIDGET_FACES[id] ?? null
}

export function preloadNoteBuiltinWidgets(types: Iterable<string>): void {
  const list = [...types]
  if (list.length === 0) return
  const jobs: Promise<unknown>[] = []
  let hasReport = false
  for (const type of list) {
    if (type.startsWith('report-')) {
      hasReport = true
      continue
    }
    const load = NOTE_WIDGET_LOADERS[type as BuiltinWidgetId]
    if (load) jobs.push(load().catch(() => {}))
  }
  if (hasReport) jobs.push(preloadReportCardsForTypes(list).catch(() => {}))
  void Promise.all(jobs)
}

export function noteBuiltinWidgets(widgetsI18n: TranslationKeys['widgets']): WidgetType[] {
  return BUILTIN_WIDGET_META.filter((meta) => builtinWidgetVisibleOn(meta, 'note')).flatMap((meta) => {
    const component = noteWidgetFace(meta.id)
    if (!component) return []
    return [{
      id: meta.id,
      name: widgetsI18n[meta.nameKey],
      defaultSize: meta.defaultSize,
      component,
      preload: () => {
        preloadNoteBuiltinWidgets([meta.id])
        return Promise.resolve()
      },
      supportedSizes: [...meta.supportedSizes],
      componentLongPress:
        meta.id.startsWith('report-') ||
        meta.id === 'social-network' ||
        meta.id === 'tapp-shortcut' ||
        meta.id === 'game-presence' ||
        undefined,
      settings:
        meta.id === 'github-repos'
          ? [
              {
                key: 'repo',
                type: 'input' as const,
                label: widgetsI18n.githubReposField,
                description: widgetsI18n.githubReposFieldHint,
                placeholder: 'owner/repo',
                defaultValue: '',
              },
            ]
          : undefined,
    }]
  })
}
