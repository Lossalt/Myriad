import assert from 'node:assert/strict'
import test from 'node:test'
import { plainHiddenCollars } from './hiddenCollar'
import { shadeHiddenBackHair } from './hiddenHair'
import { fillHiddenSkin } from './hiddenSkin'

type Paint = (x: number, y: number) => [number, number, number, number]

function image(width: number, height: number, paint: Paint) {
  const pixels = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) pixels.set(paint(x, y), (y * width + x) * 4)
  }
  return { pixels, width, height }
}

function at(out: { pixels: Uint8ClampedArray; width: number }, x: number, y: number) {
  return Array.from(out.pixels.slice((y * out.width + x) * 4, (y * out.width + x) * 4 + 4))
}

test('the face under the fringe is plain skin, its painted shadow gone', () => {
  const box = { x: 0, y: 0, w: 100, h: 100 }
  // Skin everywhere; a dark stripe of shadow up where the fringe hides it.
  const face = image(100, 100, (_, y) => (y >= 20 && y < 30 ? [120, 80, 80, 255] : [250, 220, 210, 255]))
  const fringe = image(100, 100, (_, y) => [0, 0, 0, y < 40 ? 255 : 0])
  const out = fillHiddenSkin(box, face, [{ layer: box, image: fringe }])!
  assert.ok(out)
  const [r, g, b] = at(out, 50, 25)
  assert.ok(r > 230 && g > 200 && b > 190, `under the fringe ${[r, g, b]}`)
  // What shows is untouched.
  assert.deepEqual(at(out, 50, 70), [250, 220, 210, 255])
})

test('behind the head the back hair is the hair in its own shadow', () => {
  const box = { x: 0, y: 0, w: 100, h: 100 }
  // Hair with a lighter and a darker tone, painted pale behind the face.
  const hair = image(100, 100, (x, y) => (x > 30 && x < 70 && y > 30 ? [250, 240, 245, 255] : x % 10 < 2 ? [150, 90, 120, 255] : [240, 170, 200, 255]))
  const face = image(100, 100, (x, y) => [0, 0, 0, x > 25 && x < 75 && y > 25 ? 255 : 0])
  const out = shadeHiddenBackHair(box, hair, [{ layer: box, image: face }])!
  assert.ok(out)
  const [r, g, b] = at(out, 50, 60)
  assert.ok(Math.abs(r - 150) < 15 && Math.abs(g - 90) < 15 && Math.abs(b - 120) < 15, `behind the face ${[r, g, b]}`)
  assert.deepEqual(at(out, 10, 10), [150, 90, 120, 255])
  assert.deepEqual(at(out, 15, 10), [240, 170, 200, 255])
})

test('behind the neck the top is the cloth it shows around the neck, strokes and holes gone', () => {
  const box = { x: 0, y: 0, w: 100, h: 100 }
  // A white collar; behind the neck (x 40..60, y 0..60) skin with dark strokes and a hole.
  const top = image(100, 100, (x, y) => {
    if (x >= 40 && x < 60 && y < 60) {
      if (x === 50 && y >= 20 && y < 24) return [0, 0, 0, 0]
      return x % 6 === 0 ? [30, 20, 20, 255] : [240, 205, 190, 255]
    }
    return [245, 245, 248, 255]
  })
  const neck = image(100, 100, (x, y) => [240, 205, 190, x >= 40 && x < 60 && y < 60 ? 255 : 0])
  const front = image(100, 100, (x, y) => (x >= 44 && x < 56 && y >= 50 && y < 60 ? [245, 245, 248, 255] : [0, 0, 0, 0]))
  const out = plainHiddenCollars([{ layer: box, image: top }, { layer: box, image: front, inFront: true }], box, neck)
  assert.deepEqual([...out.keys()], [0], 'the front collar over the neck shows: only the top is filled')
  const filled = out.get(0)!
  for (const [x, y] of [[48, 30], [50, 21]]) {
    const [r, g, b, a] = at(filled, x, y)
    assert.ok(r > 235 && g > 235 && b > 235 && a === 255, `behind the neck at ${x},${y}: ${[r, g, b, a]}`)
  }
  assert.deepEqual(at(filled, 20, 30), [245, 245, 248, 255])
  // Through a collar the neck shows only within its clip: below it, the top shows and stays.
  const clip = Float32Array.from([40, 0, 60, 0, 40, 40, 60, 40])
  const clipped = plainHiddenCollars([{ layer: box, image: top }], box, neck, clip).get(0)!
  assert.deepEqual(at(clipped, 49, 50), [240, 205, 190, 255])
  assert.ok(at(clipped, 48, 30)[0] > 235)
})
