import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { tuckGarmentOffLegsByReference, tuckLiningBehindLegs } from './skirtLining'

function solid(
  id: string,
  role: RasterLayer['role'],
  rgb: readonly [number, number, number],
  box: { left: number; top: number; width: number; height: number },
): RasterLayer {
  const data = new Uint8ClampedArray(box.width * box.height * 4)
  for (let i = 0; i < data.length; i += 4) data.set([...rgb, 255], i)
  return {
    id,
    role,
    sourceName: id,
    order: 0,
    side: 'left',
    group: 'body',
    ...box,
    data,
  }
}

/** Two legs 300 tall with a 10 px gap at x 40..50, and a skirt over their tops. */
function standingFigure(tab: { left: number; width: number; bottom: number }) {
  const legL = { ...solid('legwear-left', 'legwear', [250, 220, 210], { left: 10, top: 20, width: 30, height: 300 }), side: 'left' as const }
  const legR = { ...solid('legwear-right', 'legwear', [250, 220, 210], { left: 50, top: 20, width: 30, height: 300 }), side: 'right' as const }
  const skirt = { ...solid('bottomwear', 'bottomwear', [120, 80, 200], { left: 5, top: 0, width: 80, height: tab.bottom }), side: null }
  // The hem is at y 30; below it only the tab is drawn.
  for (let y = 30; y < tab.bottom; y += 1) {
    for (let x = 0; x < skirt.width; x += 1) {
      const canvasX = skirt.left + x
      if (canvasX < tab.left || canvasX >= tab.left + tab.width) skirt.data[(y * skirt.width + x) * 4 + 3] = 0
    }
  }
  return [legL, legR, skirt]
}

test('a tab of skirt hanging between the legs goes behind them', () => {
  const layers = tuckLiningBehindLegs(standingFigure({ left: 39, width: 12, bottom: 40 }))
  assert.deepEqual(layers.map((layer) => layer.id), ['bottomwear-lining', 'legwear-left', 'legwear-right', 'bottomwear'])
  const [lining, , , skirt] = layers
  // Trimmed to the tab (x 39..51, y 30..40) with the usual 2 px margin, inside the skirt's own box.
  assert.deepEqual([lining.left, lining.top, lining.width, lining.height], [37, 28, 16, 12])
  // The skirt keeps its hem, without the tab.
  assert.equal(skirt.data[(35 * skirt.width + (44 - skirt.left)) * 4 + 3], 0)
  assert.equal(skirt.data[(10 * skirt.width + (44 - skirt.left)) * 4 + 3], 255)
})

test('a skirt reaching across the legs, or a short fringe between them, stays in front', () => {
  const across = standingFigure({ left: 20, width: 50, bottom: 40 })
  assert.equal(tuckLiningBehindLegs(across), across)
  const fringe = standingFigure({ left: 41, width: 6, bottom: 32 })
  assert.equal(tuckLiningBehindLegs(fringe), fringe)
})

/** A 100x200 portrait: brown stockings on a leg at x 30..70, a coat's magenta lining over it where y < 100. */
function liningOverLeg(portraitLining: boolean) {
  const stocking = [94, 69, 74] as const
  const lining = [128, 60, 86] as const
  const leg = { ...solid('legwear-left', 'legwear', stocking, { left: 30, top: 0, width: 40, height: 200 }), side: 'left' as const }
  const coat = { ...solid('topwear', 'topwear', lining, { left: 0, top: 0, width: 100, height: 100 }), side: null }
  const data = new Uint8ClampedArray(100 * 200 * 4)
  for (let y = 0; y < 200; y += 1) {
    for (let x = 0; x < 100; x += 1) {
      const onLeg = x >= 30 && x < 70
      const colour = y < 100 && (!onLeg || portraitLining) ? lining : onLeg ? stocking : [255, 255, 255]
      data.set([...colour, 255], (y * 100 + x) * 4)
    }
  }
  return { layers: [leg, coat], reference: { width: 100, height: 200, data } }
}

test('a coat\'s lining drawn over a leg the portrait shows goes behind the leg', () => {
  const { layers, reference } = liningOverLeg(false)
  const tucked = tuckGarmentOffLegsByReference(layers, reference, () => true)
  assert.deepEqual(tucked.map((layer) => layer.id), ['topwear-lining', 'legwear-left', 'topwear'])
  const [lining, , coat] = tucked
  // The coat keeps what lies beside the leg; the lining has what lay over it.
  assert.equal(coat.data[(50 * coat.width + 50) * 4 + 3], 0)
  assert.equal(coat.data[(50 * coat.width + 10) * 4 + 3], 255)
  assert.equal(lining.data[((50 - lining.top) * lining.width + 50 - lining.left) * 4 + 3], 255)
  assert.equal(lining.role, 'topwear')
})

test('a coat the portrait shows over the leg stays in front of it', () => {
  const { layers, reference } = liningOverLeg(true)
  assert.equal(tuckGarmentOffLegsByReference(layers, reference, () => true), layers)
})
