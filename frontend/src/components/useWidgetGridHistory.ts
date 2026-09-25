import type { WidgetConfig } from './widgetGridTypes'
import { useCallback, useEffect, useRef } from 'react'
import {
  isEditableKeyTarget,
  isRedoKey,
  isUndoKey,
  pushWidgetHistory,
  redoWidgetHistory,
  sameWidgets,
  undoWidgetHistory,
} from './widgetGridHistory'

/**
 * Undo history of what the grid shows. It starts from the layout on entering
 * edit mode (or switching layout mode) and records every change, including
 * ones made outside the grid such as a newly added sticker: an undo can only
 * return to a state that was actually shown, never drop a tile it never saw.
 */
export function useWidgetGridHistory(
  isEditMode: boolean,
  widgets: WidgetConfig[],
  resetKey: string,
  onWidgetsChange?: (widgets: WidgetConfig[]) => void,
): {
  saveToHistory: (widgets: WidgetConfig[]) => void
  handleUndo: () => void
  handleRedo: () => void
} {
  const historyRef = useRef<{ history: WidgetConfig[][]; index: number }>({
    history: [],
    index: -1,
  })
  const onWidgetsChangeRef = useRef(onWidgetsChange)
  onWidgetsChangeRef.current = onWidgetsChange
  const widgetsRef = useRef(widgets)
  widgetsRef.current = widgets

  const saveToHistory = useCallback((next: WidgetConfig[]) => {
    const { history, index } = historyRef.current
    if (index >= 0 && sameWidgets(history[index], next)) return
    historyRef.current = pushWidgetHistory(history, index, next)
  }, [])

  useEffect(() => {
    historyRef.current = isEditMode
      ? { history: [widgetsRef.current], index: 0 }
      : { history: [], index: -1 }
  }, [isEditMode, resetKey])

  useEffect(() => {
    if (isEditMode) saveToHistory(widgets)
  }, [isEditMode, saveToHistory, widgets])

  const handleUndo = useCallback(() => {
    const { history, index } = historyRef.current
    const step = undoWidgetHistory(history, index)
    if (!step) return
    historyRef.current = { history, index: step.index }
    onWidgetsChangeRef.current?.(step.widgets)
  }, [])

  const handleRedo = useCallback(() => {
    const { history, index } = historyRef.current
    const step = redoWidgetHistory(history, index)
    if (!step) return
    historyRef.current = { history, index: step.index }
    onWidgetsChangeRef.current?.(step.widgets)
  }, [])

  useEffect(() => {
    if (!isEditMode) return
    const handleKeyDown = (event: KeyboardEvent) => {
      // Cmd+Z in a text field (the sticker prompt) edits the text.
      if (isEditableKeyTarget(event.target)) return
      if (isUndoKey(event)) {
        event.preventDefault()
        handleUndo()
      }
      if (isRedoKey(event)) {
        event.preventDefault()
        handleRedo()
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [handleRedo, handleUndo, isEditMode])

  return { saveToHistory, handleUndo, handleRedo }
}
