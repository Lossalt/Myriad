import type {
  Anime25DSourceReference,
  RasterLayer,
} from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { reconcileAnime25DPsd } from './psdReconciliation'

type Rgb = readonly [number, number, number]
type Box = readonly [number, number, number, number]

const SIZE = 200
const BACKDROP: Rgb = [250, 250, 250]
const SKIN: Rgb = [236, 200, 180]
const CLOTH: Rgb = [60, 110, 70]
const GEM: Rgb = [220, 40, 50]
const EARRING: Rgb = [120, 60, 200]
const INK: Rgb = [40, 30, 30]

function reference(
  paint: (fill: (box: Box, color: Rgb) => void) => void,
  backdrop: (x: number, y: number) => Rgb = () => BACKDROP,
): Anime25DSourceReference {
  const data = new Uint8ClampedArray(SIZE * SIZE * 4)
  for (let y = 0; y < SIZE; y += 1) {
    for (let x = 0; x < SIZE; x += 1) {
      data.set([...backdrop(x, y), 255], (y * SIZE + x) * 4)
    }
  }
  paint(([left, top, width, height], color) => {
    for (let y = top; y < top + height; y += 1) {
      for (let x = left; x < left + width; x += 1) {
        data.set([...color, 255], (y * SIZE + x) * 4)
      }
    }
  })
  return { width: SIZE, height: SIZE, data }
}

function layer(role: string, [left, top, width, height]: Box, color: Rgb) {
  const data = new Uint8ClampedArray(width * height * 4)
  for (let index = 0; index < data.length; index += 4) {
    data.set([...color, 255], index)
  }
  return {
    id: role,
    role,
    sourceName: role,
    order: 0,
    side: null,
    group: 'body',
    left,
    top,
    width,
    height,
    data,
  } as RasterLayer
}

/** A portrait whose layers reproduce the reference exactly. */
function portrait(fill: (box: Box, color: Rgb) => void) {
  fill([60, 40, 80, 60], SKIN)
  fill([40, 100, 120, 80], CLOTH)
}

function baseLayers() {
  return [
    layer('face', [60, 40, 80, 60], SKIN),
    layer('topwear', [40, 100, 120, 80], CLOTH),
  ]
}

test('a faithful split reconciles without defects', () => {
  const result = reconcileAnime25DPsd(baseLayers(), reference(portrait))!
  assert.equal(result.status, 'reconciled')
  assert.equal(result.backgroundKnown, true)
  assert.ok(result.agreement > 0.99)
  assert.deepEqual(result.regions, [])
})

test('art hidden under a later layer is reported as buried', () => {
  const layers = [layer('neckwear', [90, 110, 20, 20], GEM), ...baseLayers()]
  const result = reconcileAnime25DPsd(
    layers,
    reference((fill) => {
      portrait(fill)
      fill([90, 110, 20, 20], GEM)
    }),
  )!
  assert.deepEqual(
    result.regions.map(({ kind, role, area }) => ({ kind, role, area })),
    [{ kind: 'buried', role: 'neckwear', area: 400 }],
  )
})

test('illustrated art that no layer carries is reported as missing', () => {
  const result = reconcileAnime25DPsd(
    baseLayers(),
    reference((fill) => {
      portrait(fill)
      fill([46, 70, 10, 24], EARRING)
    }),
  )!
  assert.deepEqual(
    result.regions.map(({ kind, area, box }) => ({ kind, area, box })),
    [
      {
        kind: 'missing',
        area: 240,
        box: { x: 46, y: 70, width: 10, height: 24 },
      },
    ],
  )
})

test('paint over the open backdrop is spurious, over enclosed pale art a mismatch', () => {
  const layers = [
    ...baseLayers(),
    layer('front-hair', [20, 20, 20, 30], INK),
    layer('back-hair', [80, 50, 20, 20], INK),
  ]
  const result = reconcileAnime25DPsd(
    layers,
    reference((fill) => {
      portrait(fill)
      // A backdrop-coloured highlight enclosed by the face, like pale skin.
      fill([80, 50, 20, 20], BACKDROP)
    }),
  )!
  const byRole = Object.fromEntries(
    result.regions.map((region) => [region.role, region.kind]),
  )
  assert.equal(byRole['front-hair'], 'spurious')
  assert.equal(byRole['back-hair'], 'mismatch')
})

test('a PSD split from another illustration is not reconciled', () => {
  const result = reconcileAnime25DPsd(
    baseLayers(),
    reference((fill) => {
      fill([60, 40, 80, 60], INK)
      fill([40, 100, 120, 80], EARRING)
    }),
  )!
  assert.equal(result.status, 'reference-mismatch')
  assert.deepEqual(result.regions, [])
  assert.equal(result.heatmap, null)
})

test('a scenic backdrop disables the backdrop-dependent verdicts', () => {
  const result = reconcileAnime25DPsd(
    [...baseLayers(), layer('front-hair', [20, 20, 20, 30], INK)],
    reference(
      (fill) => {
        portrait(fill)
        fill([4, 60, 12, 24], EARRING)
      },
      (x, y) => [(x * 7) % 256, (y * 5) % 256, ((x + y) * 3) % 256],
    ),
  )!
  assert.equal(result.backgroundKnown, false)
  assert.ok(
    result.regions.every(
      (region) => region.kind !== 'missing' && region.kind !== 'spurious',
    ),
  )
})

test('square padding outside the illustration is never judged', () => {
  const padded = reference(portrait)
  for (let y = 0; y < SIZE; y += 1) {
    for (let x = 0; x < 30; x += 1) padded.data[(y * SIZE + x) * 4 + 3] = 0
  }
  const result = reconcileAnime25DPsd(
    [...baseLayers(), layer('handwear', [0, 120, 30, 40], INK)],
    padded,
  )!
  assert.deepEqual(result.regions, [])
  // The overview crops to the illustration, not the padding beside it.
  assert.deepEqual([result.heatmap?.width, result.heatmap?.height], [136, 152])
})
