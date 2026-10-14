import type { UprightReason } from '../api'
import { useSyncExternalStore } from 'react'

/** The worn bust's picture: the master portrait. */
export const MASTER_SLOT = 'master'

/**
 * The pictures just uploaded that should be redrawn upright, by set (or
 * `MASTER_SLOT`), with what a redraw would put right. Only an upload asks:
 * a page opened later on the same picture does not suggest it again.
 */
const pending = new Map<string, UprightReason[]>()
const listeners = new Set<() => void>()

function changed(): void {
  for (const listener of listeners) listener()
}

/** An upload was looked at; nothing to suggest when nothing is to be put right. */
export function requestUprightCheck(slot: string, reasons: readonly UprightReason[]): void {
  if (reasons.length) pending.set(slot, [...reasons])
  else if (!pending.delete(slot)) return
  changed()
}

export function settleUprightCheck(slot: string): void {
  if (pending.delete(slot)) changed()
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

/** What of this set's just uploaded picture a redraw would put right, if it should be offered. */
export function useUprightRequested(slot: string): UprightReason[] | null {
  return useSyncExternalStore(
    subscribe,
    () => pending.get(slot) ?? null,
    () => null,
  )
}
