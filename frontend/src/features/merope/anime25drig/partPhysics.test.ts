import type { Anime25DPart, Anime25DPartProfile } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { bindHangingPhysics } from './earwearPhysics'
import { movingPartIn, PENDULUM_DEFAULT, pendulumTuning, skirtTuning } from './partPhysics'

function part(patch: Partial<Anime25DPart>): Anime25DPart {
  return {
    name: 'charm',
    layer: 'recovered-objects',
    separate: true,
    box: [100, 400, 140, 520],
    anchor: [120, 400],
    bone: 'chest',
    motion: 'dangle',
    material: 'metal',
    stiffness: 0.5,
    weight: 0.5,
    ...patch,
  }
}

const profile = (parts: Anime25DPart[]): Anime25DPartProfile => ({ version: 1, source: 'ai-vision', parts })

test('a part swings by what it is made of', () => {
  const stiff = pendulumTuning(part({ stiffness: 1 }))
  const limp = pendulumTuning(part({ stiffness: 0 }))
  assert.ok(stiff.stiffness > limp.stiffness, 'stiff swings back faster')
  assert.ok(stiff.reach < limp.reach, 'and not as far')
  const ribbon = pendulumTuning(part({ motion: 'flutter', material: 'ribbon', stiffness: 0.1, weight: 0.1 }))
  const metal = pendulumTuning(part({ material: 'metal', stiffness: 0.8, weight: 0.9 }))
  assert.ok(ribbon.reach > metal.reach && ribbon.stiffness < metal.stiffness && ribbon.damping < metal.damping)
  const organza = skirtTuning(part({ layer: 'bottomwear', motion: 'sway', stiffness: 0.9 }))!
  const chiffon = skirtTuning(part({ layer: 'bottomwear', motion: 'sway', stiffness: 0.1 }))!
  assert.ok(organza.frequency > chiffon.frequency)
  assert.equal(skirtTuning(part({ motion: 'rigid' })), null)
  assert.equal(skirtTuning(null), null)
})

test('only a part that moves, drawn in the layer, is found', () => {
  const found = profile([part({ motion: 'rigid' }), part({ name: 'chain' })])
  assert.equal(movingPartIn(found, 'recovered-objects')?.name, 'chain')
  assert.equal(movingPartIn(found, 'topwear'), null)
  assert.equal(movingPartIn(undefined, 'recovered-objects'), null)
})

test('a hanging part swings from its attachment; without an inventory an earring keeps its own rule', () => {
  const anchors = { face: { x0: 0, y0: 100, x1: 200, y1: 300, cx: 300, cy: 200 } } as never
  const source = { name: 'recovered-objects', role: 'objects', x: 100, y: 400, w: 40, h: 120 } as never
  const swing = bindHangingPhysics(source, { x: 120, y: 400 }, { anchors, partProfile: profile([part({})]) }, () => null)
  assert.ok(swing, 'the charm swings')
  assert.equal(bindHangingPhysics(source, { x: 120, y: 400 }, { anchors, partProfile: undefined }, () => null), null)
  assert.equal(bindHangingPhysics(source, null, { anchors, partProfile: profile([part({})]) }, () => null), null)
  assert.deepEqual(PENDULUM_DEFAULT, { stiffness: 1, damping: 0.65, reach: 1 })
})
