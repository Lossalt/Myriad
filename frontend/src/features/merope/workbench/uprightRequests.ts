import { useSyncExternalStore } from 'react'

/** The worn bust's picture: the master portrait. */
export const MASTER_SLOT = 'master'

/**
 * The pictures just uploaded, by set (or `MASTER_SLOT`): each is read once
 * for whether it should be redrawn square to the camera. Only an upload asks;
 * a page opened later on the same picture does not read it again.
 */
const pending = new Set<string>()
const listeners = new Set<() => void>()

function changed(): void {
  for (const listener of listeners) listener()
}

export function requestUprightCheck(slot: string): void {
  pending.add(slot)
  changed()
}

export function settleUprightCheck(slot: string): void {
  if (pending.delete(slot)) changed()
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

/** Whether this set's picture was just uploaded and is still to be read. */
export function useUprightRequested(slot: string): boolean {
  return useSyncExternalStore(
    subscribe,
    () => pending.has(slot),
    () => false,
  )
}
