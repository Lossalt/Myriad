import type { WidgetConfig } from './widgetGridTypes'

export const WIDGET_HISTORY_LIMIT = 20

export function pushWidgetHistory(
  history: WidgetConfig[][],
  index: number,
  next: WidgetConfig[],
  limit = WIDGET_HISTORY_LIMIT,
): { history: WidgetConfig[][]; index: number } {
  const trimmed = history.slice(0, index + 1)
  trimmed.push(next)
  if (trimmed.length > limit) {
    trimmed.shift()
    return { history: trimmed, index }
  }
  return { history: trimmed, index: index + 1 }
}

export function undoWidgetHistory(
  history: WidgetConfig[][],
  index: number,
): { index: number; widgets: WidgetConfig[] } | null {
  if (index <= 0) return null
  return { index: index - 1, widgets: history[index - 1] }
}

export function redoWidgetHistory(
  history: WidgetConfig[][],
  index: number,
): { index: number; widgets: WidgetConfig[] } | null {
  if (index >= history.length - 1) return null
  return { index: index + 1, widgets: history[index + 1] }
}

/** Same tiles in the same order, compared by value (undo re-renders copies). */
export function sameWidgets(a: WidgetConfig[] | undefined, b: WidgetConfig[]): boolean {
  if (a === b) return true
  if (!a || a.length !== b.length) return false
  return JSON.stringify(a) === JSON.stringify(b)
}

/** Keys typed into a text field belong to that field, not the grid. */
export function isEditableKeyTarget(target: EventTarget | null): boolean {
  const element = target as {
    isContentEditable?: boolean
    closest?: (selector: string) => unknown
  } | null
  if (!element) return false
  if (element.isContentEditable) return true
  return typeof element.closest === 'function'
    && element.closest('input, textarea, select, [contenteditable=""], [contenteditable="true"]') != null
}

export function isUndoKey(event: {
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
  key: string
}): boolean {
  return (event.ctrlKey || event.metaKey) && event.key === 'z' && !event.shiftKey
}

export function isRedoKey(event: {
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
  key: string
}): boolean {
  return (
    (event.ctrlKey || event.metaKey) &&
    ((event.shiftKey && event.key === 'z') || event.key === 'y')
  )
}
