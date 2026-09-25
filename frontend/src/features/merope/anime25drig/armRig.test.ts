import assert from 'node:assert/strict'
import test from 'node:test'
import { bindArmRig, bindArmRigMesh } from './armRig'

const ANCHORS = {
  face: { x0: 300, y0: 150, x1: 700, y1: 650, cx: 500, cy: 400 },
  neckPivot: { x: 500, y: 740 },
  neckBottom: 770,
}

type Paint = (x: number, y: number) => boolean

function sleeve(layer: { x: number; y: number; w: number; h: number }, paint: Paint) {
  const pixels = new Uint8ClampedArray(layer.w * layer.h * 4)
  for (let y = 0; y < layer.h; y++) {
    for (let x = 0; x < layer.w; x++) {
      if (paint(layer.x + x, layer.y + y)) pixels[(y * layer.w + x) * 4 + 3] = 255
    }
  }
  return { pixels, width: layer.w, height: layer.h }
}

// A hanging image-left sleeve: a 160px-wide column from the shoulder to the crop.
const LEFT = { x: 120, y: 780, w: 220, h: 540, side: 'L' as const }
const hanging: Paint = (x, y) => x >= 150 && x < 310 && y >= 790

test('the joint sits inside the sleeve top nearest the anatomical shoulder', () => {
  const rig = bindArmRig(LEFT, sleeve(LEFT, hanging), ANCHORS, 1320)!
  assert.equal(rig.outward, 1)
  assert.ok(rig.pivotX > 150 && rig.pivotX < 310, `${rig.pivotX}`)
  assert.ok(rig.pivotY > 790 && rig.pivotY < 900, `${rig.pivotY}`)
  assert.equal(rig.scale, 1)
  assert.equal(rig.cutY, 1320)
})

test('the image-right sleeve swings the other way', () => {
  const right = { ...LEFT, x: 660, side: 'R' as const }
  const rig = bindArmRig(right, sleeve(right, (x, y) => x >= 690 && x < 850 && y >= 790), ANCHORS, 1320)!
  assert.equal(rig.outward, -1)
  assert.ok(rig.pivotX > 690 && rig.pivotX < 850)
})

test('a sleeve that ends above the crop has no cut to slide along', () => {
  const short = { ...LEFT, h: 300 }
  const rig = bindArmRig(short, sleeve(short, (x, y) => hanging(x, y) && y < 1060), ANCHORS, 1320)!
  assert.equal(rig.cutY, null)
})

test('a tapered sleeve tip reaching the crop edge is not taken for a cut', () => {
  const rig = bindArmRig(LEFT, sleeve(LEFT, (x, y) => hanging(x, y) && (y < 1300 || x < 156)), ANCHORS, 1320)!
  assert.equal(rig.cutY, null)
})

test('a hand raised beside the face is not mistaken for the shoulder', () => {
  const posed = { x: 120, y: 500, w: 260, h: 820, side: 'L' as const }
  // The forearm rises from the shoulder to a hand level with the chin.
  const paint: Paint = (x, y) =>
    (x >= 150 && x < 310 && y >= 790) || (x >= 300 && x < 380 && y >= 520 && y < 800)
  const rig = bindArmRig(posed, sleeve(posed, paint), ANCHORS, 1320)!
  assert.ok(rig.pivotY >= ANCHORS.neckBottom - 25, `${rig.pivotY}`)
  assert.ok(rig.scale < 1)
})

test('no pixels, no side, or an empty drawing bind no joint', () => {
  assert.equal(bindArmRig(LEFT, null, ANCHORS, 1320), null)
  assert.equal(bindArmRig({ ...LEFT, side: null }, sleeve(LEFT, hanging), ANCHORS, 1320), null)
  assert.equal(bindArmRig(LEFT, sleeve(LEFT, () => false), ANCHORS, 1320), null)
})

test('mesh weights are zero at the joint and whole beyond the shoulder', () => {
  const rig = bindArmRig(LEFT, sleeve(LEFT, hanging), ANCHORS, 1320)!
  const rest = new Float32Array([rig.pivotX, rig.pivotY, rig.pivotX, rig.pivotY + rig.radius * 0.8, 200, 1300])
  const mesh = bindArmRigMesh(rig, rest)
  assert.equal(mesh.jointVertex, 0)
  assert.equal(mesh.weights[0], 0)
  assert.ok(mesh.weights[1] > 0 && mesh.weights[1] < 1)
  assert.equal(mesh.weights[2], 1)
})
