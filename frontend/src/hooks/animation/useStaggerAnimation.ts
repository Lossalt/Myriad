import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { coordinator } from './coordinator'
import { AnimationPriority, AnimationState } from './types'

interface UseStaggerAnimationOptions {
  groupId: string
  index: number
  baseDelay?: number
  waitForPage?: boolean
  enabled?: boolean
  /** Defer scheduling until content can be shown; a late release keeps the original slot time. */
  hold?: boolean
  priority?: AnimationPriority
}

let staggerIdCounter = 0

/** Admission is one-shot; ownership lasts until completion or unmount. */
export function useStaggerAnimation({
  groupId,
  index,
  baseDelay,
  waitForPage = true,
  enabled = true,
  hold = false,
  priority = AnimationPriority.ELEMENT,
}: UseStaggerAnimationOptions) {
  const pageRevision = useSyncExternalStore(
    coordinator.subscribePage,
    coordinator.getPageRevision,
    coordinator.getPageRevision,
  )
  const idRef = useRef('')
  if (!idRef.current) idRef.current = `stagger-${groupId}-${++staggerIdCounter}`
  const id = idRef.current
  const [canAnimate, setCanAnimate] = useState(!enabled)
  const admitted = useRef(!enabled)
  const unsubscribe = useRef<(() => void) | null>(null)
  const [mountedAt] = useState(() => performance.now())
  const held = useRef(false)
  const coordinatedDelay = coordinator.getStaggerDelay(index, baseDelay)

  const cancel = useCallback(() => {
    if (!unsubscribe.current) return
    // Release before removing the listener so its state is also removed.
    coordinator.skip(id)
    unsubscribe.current()
    unsubscribe.current = null
  }, [id])

  useEffect(() => {
    if (!enabled || !waitForPage) {
      cancel()
      admitted.current = true
      setCanAnimate(true)
      return
    }
    // Reordering and preference changes only affect entrances still waiting.
    // Running cards keep both their visible pose and their concurrency slot.
    if (admitted.current) return
    if (hold) {
      held.current = true
      return
    }
    cancel()
    // Time spent held counts toward the stagger slot: never earlier, never later than needed.
    const delay = held.current
      ? Math.max(0, coordinatedDelay - (performance.now() - mountedAt))
      : coordinatedDelay
    coordinator.schedule({ id, priority, delay })
    unsubscribe.current = coordinator.subscribe(id, (state) => {
      if (state !== AnimationState.READY || admitted.current) return
      admitted.current = true
      coordinator.markRunning(id)
      setCanAnimate(true)
    })
  }, [cancel, coordinatedDelay, enabled, hold, id, mountedAt, pageRevision, priority, waitForPage])

  useEffect(() => cancel, [cancel])

  const onComplete = useCallback(() => {
    if (admitted.current && unsubscribe.current) {
      coordinator.markCompleted(id)
      unsubscribe.current()
      unsubscribe.current = null
    }
  }, [id])

  return { canAnimate, onComplete }
}
