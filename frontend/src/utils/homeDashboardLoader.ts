import type { HomeDashboardLayouts, HomeLayoutMode } from './homeLayout'
import {
  homeLayoutsHaveTiles,
  layoutsForFirstPaint,
  parseDashboardLayoutJson,
  parseHomeLayoutMode,
} from './homeLayout'

interface DashboardLoadOptions {
  read: () => Promise<Record<string, unknown>>
  preload: (layouts: HomeDashboardLayouts, mode: HomeLayoutMode) => Promise<unknown>
  fallback: () => HomeDashboardLayouts
  generation: { current: number }
  mode: (mode: HomeLayoutMode) => void
  title: (title: string) => void
  raw: (layouts: HomeDashboardLayouts) => void
  apply: (layouts: HomeDashboardLayouts) => void
  error: (error: unknown) => Promise<string>
  notify: (message: string) => void
  timeoutMs?: number
}

/** Own the first-paint load, including its deadline and all late continuations. */
export function startHomeDashboardLoad(options: DashboardLoadOptions): () => void {
  let active = true
  let timer: ReturnType<typeof setTimeout> | undefined
  let release: (() => void) | undefined
  const apply = async (layouts: HomeDashboardLayouts, mode: HomeLayoutMode) => {
    if (!active) return
    const generation = ++options.generation.current
    try {
      await new Promise<void>((resolve, reject) => {
        release = resolve
        timer = setTimeout(resolve, options.timeoutMs ?? 3000)
        Promise.try(() => options.preload(layouts, mode)).then(() => resolve(), reject)
      })
    } finally {
      clearTimeout(timer)
      timer = undefined
      release = undefined
    }
    if (active && generation === options.generation.current) options.apply(layouts)
  }
  const report = async (error: unknown) => {
    if (!active) return
    const message = await options.error(error)
    if (active) options.notify(message)
  }
  const load = async () => {
    try {
      const data = await options.read()
      if (!active) return
      const mode = parseHomeLayoutMode(data.dashboard_layout_mode)
      options.mode(mode)
      options.title(typeof data.dashboard_title === 'string' && data.dashboard_title ? data.dashboard_title : 'Dashboard')
      let layouts = options.fallback()
      if (data.dashboard_layout) {
        try {
          const parsed = parseDashboardLayoutJson(String(data.dashboard_layout))
          options.raw(parsed)
          if (homeLayoutsHaveTiles(parsed)) layouts = layoutsForFirstPaint(parsed)
        } catch (error) {
          await report(error)
        }
      }
      await apply(layouts, mode)
    } catch (error) {
      await report(error)
      if (!active) return
      options.mode('standard')
      options.title('Dashboard')
      await apply(options.fallback(), 'standard')
    }
  }
  // Report presentation/preload failures without leaving a detached rejection.
  void load().catch(error => {
    if (active) console.error('Home dashboard load failed:', error)
  })
  return () => {
    active = false
    clearTimeout(timer)
    release?.()
  }
}
