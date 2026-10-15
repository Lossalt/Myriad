import { coordinator } from './coordinator'

export { useIdleEffect } from './atomicHooks'

export { coordinator } from './coordinator'

export {
  isPageVisible,
  observeResize as observeResizeAtomic,
  onVisibility,
  scheduleTask,
  startPage,
} from './core'

export { pageIdFromPath } from './pageId'

export {
  useLibraryIntersectionObserver,
  useLibraryScheduler,
} from './pages/library'

export {
  getPhantasiTransition,
  phantasiAnimationPresets,
  usePhantasiAnimationConfig,
} from './pages/phantasi'

export { useTappStagger } from './pages/tapp'

export { useLoopAnimation } from './useLoopAnimation'
export { usePageReady } from './usePageReady'

export { usePageTransition } from './usePageTransition'

export { useRouteScheduler } from './useRouteScheduler'
export { useStaggerAnimation } from './useStaggerAnimation'

export { useVisibilityInterval } from './useVisibilityPause'

export { useWidgetResizeObserver } from './useWidgetResizeObserver'

export function configureAnimationCoordinator(
  config: Partial<import('./types').CoordinatorConfig>,
) {
  coordinator.updateConfig(config)
}

export function startFpsMonitor() {
  coordinator.startFpsMonitor()
}

export function stopFpsMonitor() {
  coordinator.stopFpsMonitor()
}

export function isLowFps(): boolean {
  return coordinator.isLowFps()
}

export function getFrameStats() {
  return coordinator.getFrameStats()
}

export function batchRead(callback: () => void): void {
  coordinator.batchRead(callback)
}

export function batchWrite(callback: () => void): void {
  coordinator.batchWrite(callback)
}

export function observeResize(
  element: Element,
  callback: (entry: ResizeObserverEntry) => void,
): () => void {
  return coordinator.observeResize(element, callback)
}

export function getCachedSize(
  element: Element,
): { width: number; height: number } | null {
  return coordinator.getCachedSize(element)
}

export function scheduleIdleTask(
  id: string,
  task: () => void,
  options?: {
    timeout?: number
    priority?: 'low' | 'normal' | 'high'
    dedupe?: boolean
  },
): () => void {
  return coordinator.scheduleIdleTask(id, task, options)
}

export function cancelIdleTask(id: string): boolean {
  return coordinator.cancelIdleTask(id)
}
