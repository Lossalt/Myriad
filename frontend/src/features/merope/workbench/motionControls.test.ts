import assert from 'node:assert/strict'
import test from 'node:test'
import zh from '../../../i18n/merope.zh-CN.json' with { type: 'json' }
import { FACE_PRESETS } from '../events/performanceContract'
import { EXPRESSION_GROUPS, FACE_PRESET_BUTTONS, facePresetDriver, facePresetLabel, PRESETS } from './motionControls'

test('every look the director can pick is a named button in the workbench', () => {
  assert.deepEqual(FACE_PRESET_BUTTONS.map((preset) => preset.id), Object.keys(FACE_PRESETS))
  for (const { id, driver } of FACE_PRESET_BUTTONS) {
    assert.notEqual(facePresetLabel(zh as never, id), id, id)
    assert.ok(Object.keys(driver).length > 0, id)
  }
})

test('a look plays on the drivers at its tiers, as a directive would', () => {
  const content = facePresetDriver('content')
  assert.equal(content.eyeOpenL, 0)
  assert.equal(content.eyeSmile, 1)
  assert.equal(content.mouthForm, 0.65)
  assert.equal(facePresetDriver('humming').accentNotes, 1)
  assert.equal(facePresetDriver('blushing').accentBlush, 0.65)
  // Looking down is a positive driver eyeY.
  assert.ok((facePresetDriver('downcast').eyeY ?? 0) > 0)
})

test('every expression is in exactly one group, by whether it moves', () => {
  const ids = EXPRESSION_GROUPS.flatMap((group) => group.buttons.map((button) => `${button.kind}:${button.id}`))
  assert.equal(new Set(ids).size, ids.length)
  assert.deepEqual(
    new Set(ids),
    new Set([...PRESETS.map((preset) => `drawn:${preset.id}`), ...Object.keys(FACE_PRESETS).map((id) => `face:${id}`)]),
  )
  for (const group of EXPRESSION_GROUPS) assert.ok(zh[group.title as keyof typeof zh], group.title)
})
