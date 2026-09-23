import type { EntrancePhase } from '../lib/motionEntrance'
import { useCallback, useEffect, useState } from 'react'
import { useStaggerAnimation } from './animation/useStaggerAnimation'

/** A card whose content never becomes presentable still enters, as it did under the grid-wide deadline. */
export const WIDGET_CONTENT_HOLD_MS = 3000

/**
 * Hold a card's entrance until its module has loaded and its content has committed,
 * so each card enters with content instead of every card waiting for the slowest.
 */
export function useWidgetContentHold(preload: (() => Promise<unknown>) | undefined, active: boolean) {
  const [loaded, setLoaded] = useState(!preload)
  const [presented, setPresented] = useState(false)
  const [expired, setExpired] = useState(false)
  const hold = active && !expired && !(loaded && presented)

  useEffect(() => {
    if (loaded || !preload) return
    let alive = true
    void Promise.resolve(preload()).catch(() => {}).finally(() => {
      if (alive) setLoaded(true)
    })
    return () => {
      alive = false
    }
  }, [loaded, preload])

  useEffect(() => {
    if (!active) return
    const timer = setTimeout(setExpired, WIDGET_CONTENT_HOLD_MS, true)
    return () => clearTimeout(timer)
  }, [active])

  const onPresentable = useCallback(() => setPresented(true), [])
  return { hold, onPresentable }
}

/** Own the grid's one-shot admission/completion lifecycle, not content policy. */
export function useWidgetEntrance(index: number, disabled: boolean, hold = false) {
  const [complete, setComplete] = useState(disabled)
  const { canAnimate, onComplete: completeSlot } = useStaggerAnimation({
    groupId: 'widget-grid',
    index,
    baseDelay: 115,
    enabled: !disabled,
    hold,
  })

  // Content already shown in edit/reduced mode must not replay on a preference change.
  useEffect(() => {
    if (disabled) setComplete(true)
  }, [disabled])

  const onComplete = useCallback(() => {
    completeSlot()
    if (canAnimate) setComplete(true)
  }, [canAnimate, completeSlot])

  const phase: EntrancePhase = disabled
    ? 'disabled'
    : complete
      ? 'complete'
      : canAnimate
        ? 'running'
        : 'waiting'

  return { phase, canAnimate, onComplete }
}
