import type { WidgetConfig } from './widgetGridTypes'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isEditableKeyTarget,
  isRedoKey,
  isUndoKey,
  pushWidgetHistory,
  redoWidgetHistory,
  sameWidgets,
  undoWidgetHistory,
} from './widgetGridHistory'

const a: WidgetConfig[] = [
  { id: 'a', type: 'weather', size: '2x2', position: { x: 0, y: 0 } },
]
const b: WidgetConfig[] = [
  { id: 'b', type: 'weather', size: '2x2', position: { x: 2, y: 0 } },
]
const c: WidgetConfig[] = [
  { id: 'c', type: 'weather', size: '2x2', position: { x: 4, y: 0 } },
]

describe('pushWidgetHistory', () => {
  it('drops redo entries after a new edit', () => {
    const stacked = pushWidgetHistory([a, b], 0, c)
    assert.deepEqual(stacked.history, [a, c])
    assert.equal(stacked.index, 1)
  })

  it('drops the oldest snapshot once the limit is reached', () => {
    const history = Array.from({ length: 20 }, (_, index) => [
      {
        id: `w${index}`,
        type: 'weather',
        size: '2x2' as const,
        position: { x: 0, y: 0 },
      },
    ])
    const next = pushWidgetHistory(history, 19, c, 20)
    assert.equal(next.history.length, 20)
    assert.equal(next.index, 19)
    assert.equal(next.history[0][0].id, 'w1')
    assert.equal(next.history[19][0].id, 'c')
  })
})

describe('undo and redo', () => {
  it('walks the stack and stops at the ends', () => {
    const history = [a, b, c]
    assert.deepEqual(undoWidgetHistory(history, 2), { index: 1, widgets: b })
    assert.equal(undoWidgetHistory(history, 0), null)
    assert.deepEqual(redoWidgetHistory(history, 0), { index: 1, widgets: b })
    assert.equal(redoWidgetHistory(history, 2), null)
  })

  it('recognizes the edit-mode undo and redo chords', () => {
    assert.equal(
      isUndoKey({ ctrlKey: true, metaKey: false, shiftKey: false, key: 'z' }),
      true,
    )
    assert.equal(
      isRedoKey({ ctrlKey: true, metaKey: false, shiftKey: true, key: 'z' }),
      true,
    )
    assert.equal(
      isRedoKey({ ctrlKey: false, metaKey: true, shiftKey: false, key: 'y' }),
      true,
    )
    assert.equal(
      isUndoKey({ ctrlKey: true, metaKey: false, shiftKey: true, key: 'z' }),
      false,
    )
  })
})

describe('sameWidgets', () => {
  it('compares by value so an undo re-render is not a new step', () => {
    assert.equal(sameWidgets(a, structuredClone(a)), true)
    assert.equal(sameWidgets(a, b), false)
    assert.equal(sameWidgets(undefined, a), false)
    assert.equal(sameWidgets(a, [...a, ...b]), false)
  })
})

describe('isEditableKeyTarget', () => {
  const element = (match: boolean, isContentEditable = false) => ({
    isContentEditable,
    closest: () => (match ? {} : null),
  }) as unknown as EventTarget

  it('leaves Cmd+Z in text fields to the field', () => {
    assert.equal(isEditableKeyTarget(element(true)), true)
    assert.equal(isEditableKeyTarget(element(false, true)), true)
  })

  it('lets the grid handle keys elsewhere', () => {
    assert.equal(isEditableKeyTarget(element(false)), false)
    assert.equal(isEditableKeyTarget(null), false)
  })
})
