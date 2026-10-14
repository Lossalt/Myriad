import type { Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'
import { dilate, erode, insideDistance } from './pixelMasks'

type Box = Pick<Anime25DPlaybackLayer, 'x' | 'y' | 'w' | 'h'>

export interface CollarGarment {
  layer: Box
  image: CroppedLayerPixels
  /** In front of the neck (the front collar): its cloth is read, but what of it lies over the neck shows. */
  inFront?: boolean
}

/** Covered: the neck hides it. */
const COVERED = 128
/** Drawn: a garment pixel the neck hides, not empty. */
const DRAWN = 64
/** The cloth is read in a ring this share of the neck's width around what the neck hides. */
const RING = 0.08
/** Holes this share of the neck's width across, in what the neck hides of a garment, are closed. */
const HOLE = 0.04
/** Of the ring, what lies within this colour distance of its middle is the cloth there, not an outline or a shadow. */
const CLOTH_DISTANCE = 48
const MIN_SAMPLES = 40
/** The new cloth blends into the old over this many canvas pixels inside what is hidden. */
const FEATHER = 1.5

/**
 * What a garment has behind the neck is the inside of its collar, which the
 * picture never shows. A decomposition paints it as anything: the neck's own
 * skin with stray strokes, a grey band, holes. Turning the head carries the
 * neck off part of it, and that shows beside the neck. The garment's layers
 * behind it (the top and the back of a collar split from it) take the cloth
 * the garment shows right around the neck instead, the front collar's too, as
 * one smooth colour field, blended into what was there at the edge; small
 * holes close. Nothing that shows at rest changes.
 * Returns the garments' new pixels by index; empty when the neck hides none
 * of them or too little cloth shows near. Through a collar the neck is drawn
 * only within its clip (rows of left and right edges at rest), so only what
 * lies there is hidden.
 */
export function plainHiddenCollars(
  garments: readonly CollarGarment[],
  neck: Box,
  neckImage: CroppedLayerPixels,
  clip?: Float32Array,
): Map<number, CroppedLayerPixels> {
  const out = new Map<number, CroppedLayerPixels>()
  if (neckImage.pixels.length !== neckImage.width * neckImage.height * 4) return out
  const neckAlpha = (canvasX: number, canvasY: number) => {
    if (clip && !insideClip(clip, canvasX, canvasY)) return 0
    const nx = Math.floor(((canvasX - neck.x) / neck.w) * neckImage.width)
    const ny = Math.floor(((canvasY - neck.y) / neck.h) * neckImage.height)
    if (nx < 0 || ny < 0 || nx >= neckImage.width || ny >= neckImage.height) return 0
    return neckImage.pixels[(ny * neckImage.width + nx) * 4 + 3]
  }
  const hiddenOf: Array<Uint8Array | null> = []
  // The cloth right around the neck, from every garment that shows there: x, y (as shares of the neck's box), rgb.
  const samples: number[][] = []
  for (const { layer, image, inFront } of garments) {
    const { width, height, pixels } = image
    if (width < 4 || height < 4 || pixels.length !== width * height * 4) {
      hiddenOf.push(null)
      continue
    }
    const pixelX = layer.w / width
    const pixelY = layer.h / height
    const covered = new Uint8Array(width * height)
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) {
        if (neckAlpha(layer.x + (x + 0.5) * pixelX, layer.y + (y + 0.5) * pixelY) >= COVERED) covered[y * width + x] = 1
      }
    }
    const drawn = new Uint8Array(width * height)
    for (let p = 0; p < width * height; p++) drawn[p] = covered[p] && pixels[p * 4 + 3] > DRAWN ? 1 : 0
    const hole = Math.max(1, Math.round((HOLE * neck.w) / pixelX))
    const closed = erode(dilate(drawn, width, height, hole), width, height, hole)
    const hidden = new Uint8Array(width * height)
    let any = false
    for (let p = 0; p < width * height; p++) {
      if (covered[p] && closed[p]) {
        hidden[p] = 1
        any = true
      }
    }
    hiddenOf.push(any && !inFront ? hidden : null)
    const ring = dilate(covered, width, height, Math.max(1, Math.round((RING * neck.w) / pixelX)))
    for (let p = 0; p < width * height; p++) {
      if (!ring[p] || covered[p] || pixels[p * 4 + 3] <= 250) continue
      const x = p % width
      const y = (p - x) / width
      samples.push([
        (layer.x + (x + 0.5) * pixelX - neck.x) / neck.w,
        (layer.y + (y + 0.5) * pixelY - neck.y) / neck.h,
        pixels[p * 4],
        pixels[p * 4 + 1],
        pixels[p * 4 + 2],
      ])
    }
  }
  if (!hiddenOf.some(Boolean) || samples.length < MIN_SAMPLES) return out
  const middle = [2, 3, 4].map((k) => median(samples.map((sample) => sample[k])))
  const cloth = samples.filter((sample) => Math.hypot(
    sample[2] - middle[0],
    sample[3] - middle[1],
    sample[4] - middle[2],
  ) <= CLOTH_DISTANCE)
  if (cloth.length < MIN_SAMPLES) return out
  const field = [2, 3, 4].map((k) => fitPlane(cloth, k))
  garments.forEach(({ layer, image }, index) => {
    const hidden = hiddenOf[index]
    if (!hidden) return
    const { width, height, pixels } = image
    const pixelX = layer.w / width
    const pixelY = layer.h / height
    const distance = insideDistance(hidden, width, height)
    const feather = Math.max(1, FEATHER / pixelX)
    const next = new Uint8ClampedArray(pixels)
    for (let p = 0; p < width * height; p++) {
      if (!hidden[p]) continue
      const x = p % width
      const y = (p - x) / width
      const u = (layer.x + (x + 0.5) * pixelX - neck.x) / neck.w
      const v = (layer.y + (y + 0.5) * pixelY - neck.y) / neck.h
      const weight = Math.min(1, distance[p] / feather)
      for (let channel = 0; channel < 3; channel++) {
        const plane = field[channel]
        const value = Math.max(plane.low, Math.min(plane.high, plane.a + plane.b * u + plane.c * v))
        next[p * 4 + channel] = Math.round(pixels[p * 4 + channel] * (1 - weight) + value * weight)
      }
      next[p * 4 + 3] = Math.round(pixels[p * 4 + 3] + (255 - pixels[p * 4 + 3]) * weight)
    }
    out.set(index, { pixels: next, width, height })
  })
  return out
}

/** Within the clip's rows (left x, y, right x, y) at rest, between consecutive rows. */
function insideClip(rows: Float32Array, x: number, y: number): boolean {
  for (let row = 0; row + 7 < rows.length; row += 4) {
    const y0 = rows[row + 1]
    const y1 = rows[row + 5]
    if (y < Math.min(y0, y1) || y > Math.max(y0, y1)) continue
    const t = y1 === y0 ? 0 : (y - y0) / (y1 - y0)
    const left = rows[row] + (rows[row + 4] - rows[row]) * t
    const right = rows[row + 2] + (rows[row + 6] - rows[row + 2]) * t
    if (x >= left && x <= right) return true
  }
  return false
}

/** value ≈ a + b·u + c·v over the samples' channel k, clamped to what they span. */
function fitPlane(samples: readonly number[][], k: number): { a: number; b: number; c: number; low: number; high: number } {
  let n = 0
  let su = 0
  let sv = 0
  let suu = 0
  let svv = 0
  let suv = 0
  let sz = 0
  let suz = 0
  let svz = 0
  for (const sample of samples) {
    const [u, v] = sample
    const z = sample[k]
    n++
    su += u
    sv += v
    suu += u * u
    svv += v * v
    suv += u * v
    sz += z
    suz += u * z
    svz += v * z
  }
  const [a, b, c] = solve3([[n, su, sv], [su, suu, suv], [sv, suv, svv]], [sz, suz, svz])
  const values = samples.map((sample) => sample[k])
  return { a, b, c, low: quantile(values, 0.05), high: quantile(values, 0.95) }
}

function solve3(m: number[][], v: number[]): [number, number, number] {
  const det = (q: number[][]) =>
    q[0][0] * (q[1][1] * q[2][2] - q[1][2] * q[2][1]) -
    q[0][1] * (q[1][0] * q[2][2] - q[1][2] * q[2][0]) +
    q[0][2] * (q[1][0] * q[2][1] - q[1][1] * q[2][0])
  const d = det(m)
  if (Math.abs(d) < 1e-9) return [v[0] / Math.max(1, m[0][0]), 0, 0]
  const replace = (column: number) => m.map((row, r) => row.map((cell, j) => (j === column ? v[r] : cell)))
  return [det(replace(0)) / d, det(replace(1)) / d, det(replace(2)) / d]
}

function median(values: number[]): number {
  return quantile(values, 0.5)
}

function quantile(values: readonly number[], share: number): number {
  const sorted = values.toSorted((left, right) => left - right)
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.floor(share * sorted.length)))]
}
