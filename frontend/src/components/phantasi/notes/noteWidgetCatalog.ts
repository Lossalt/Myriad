import type { WidgetType } from '../../widgetGridTypes'
import { useMemo } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import useTappWidgets from '../../../hooks/useTappWidgets'
import { noteBuiltinWidgets, preloadNoteBuiltinWidgets } from './noteBuiltinWidgets'

/** 编辑器整段都要目录（写栏也能插）；阅读器有占位才拉。 */
export function useNoteWidgetCatalog(enabled = true): WidgetType[] {
  const { t } = useI18n()
  const builtins = useMemo(
    () => (enabled ? noteBuiltinWidgets(t.widgets) : []),
    [enabled, t.widgets],
  )
  const { tappWidgets } = useTappWidgets(enabled)
  return useMemo(() => [...builtins, ...tappWidgets], [builtins, tappWidgets])
}

/** 水合前先拉 chunk，避免预览 / 阅读器先空壳再闪出来。 */
export function preloadNoteWidgets(types: Iterable<string>): void {
  preloadNoteBuiltinWidgets(types)
}
