import type { HomeDashboardLayouts, HomeLayoutMode } from '../utils/homeLayout'
import { preloadBuiltinWidgets } from '../components/widgets/builtinWidgets'
import {
  homeLayoutsHaveTiles,
  homeWidgetsForView,
  layoutsForFirstPaint,
  parseDashboardLayoutJson,
  parseHomeLayoutMode,
} from '../utils/homeLayout'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { VIEWPORT_MQ } from '../utils/viewportBands'
import { runWhenIdle } from '../utils/yieldToMain'

function afterPageLoad(task: () => void): void {
  if (document.readyState === 'complete') runWhenIdle(task)
  else window.addEventListener('load', () => runWhenIdle(task), { once: true })
}

/**
 * Cards hold their own entrances until their content is ready, so loading order is
 * what the viewer sees first. The lead card in reading order loads alone (it carries
 * the LCP), then the other shown cards. The hidden layout is only reachable from the
 * desktop band's mode switch, so it warms there alone, after the page has loaded.
 * Imports are shared, so repeating this is free.
 */
export function preloadHomeWidgets(layouts: HomeDashboardLayouts, mode: HomeLayoutMode): Promise<void> {
  const desktop = matchMedia(VIEWPORT_MQ.desktop).matches
  const [lead, ...rest] = homeWidgetsForView(layouts, mode, desktop)
    .toSorted((a, b) => a.position.y - b.position.y || a.position.x - b.position.x)
    .map(widget => widget.type)
  return preloadBuiltinWidgets(lead ? [lead] : [])
    .then(() => preloadBuiltinWidgets(rest))
    .then(() => {
      if (!desktop) return
      afterPageLoad(() => {
        void preloadBuiltinWidgets([...layouts.standard, ...layouts.free].map(widget => widget.type))
      })
    })
}

/** Start from the landing request, before Home mounts; Home's loader repeats it with the same imports. */
export async function warmHomeWidgets(): Promise<void> {
  const data = await getUIConfigDeduped() as Record<string, unknown>
  if (!data.dashboard_layout) return
  const parsed = parseDashboardLayoutJson(String(data.dashboard_layout))
  if (!homeLayoutsHaveTiles(parsed)) return
  await preloadHomeWidgets(layoutsForFirstPaint(parsed), parseHomeLayoutMode(data.dashboard_layout_mode))
}
