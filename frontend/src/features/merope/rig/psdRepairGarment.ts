import type { Anime25DSourceReference } from './anime25dImportTypes'
import type { Anime25DPsdAnalysis } from './psdReconciliation'
import { closeMask } from './psdRepairMasks'

/**
 * Hair hanging over a lifted garment leaves strand-shaped gaps in it, up to
 * twice this across. A strand of a layer drawn under the garment (the back
 * hair) shows through its gap only at rest: once either moves, the gap shows
 * the backdrop. So the garment takes what the portrait has there, the strand
 * drawn on it. Under a layer drawn over the garment (a front lock hanging
 * down) the gap is the garment's own hidden part, filled with its colours, so
 * the lock can swing off it.
 */
const GARMENT_GAP = 12

/**
 * Closes the gaps hair leaves in a lifted garment, adding to `pixels` (keyed
 * by canvas index). `under` is the last of `visible` drawn under the garment.
 */
export function closeGarmentGaps(
  analysis: Readonly<Anime25DPsdAnalysis>,
  shown: readonly number[],
  under: number,
  reference: Readonly<Anime25DSourceReference>,
  pixels: Map<number, readonly [number, number, number, number]>,
): void {
  const { bounds } = analysis
  const mask = new Uint8Array(bounds.width * bounds.height)
  for (const target of shown) mask[target] = 1
  const closed = closeMask(mask, bounds.width, bounds.height, GARMENT_GAP)
  const hidden: number[] = []
  for (let target = 0; target < closed.length; target += 1) {
    if (!closed[target] || mask[target]) continue
    const x = bounds.x0 + (target % bounds.width)
    const y = bounds.y0 + Math.floor(target / bounds.width)
    const offset = (y * reference.width + x) * 4
    if (reference.data[offset + 3] < 250) continue
    const key = y * reference.width + x
    if (analysis.top[target] <= under) {
      pixels.set(key, [reference.data[offset], reference.data[offset + 1], reference.data[offset + 2], 255])
    } else {
      hidden.push(key)
    }
  }
  fillFromNeighbours(hidden, reference.width, pixels)
}

/** Grows the known colours into `keys`, a ring at a time, each the mean of its known neighbours. */
function fillFromNeighbours(
  keys: readonly number[],
  width: number,
  pixels: Map<number, readonly [number, number, number, number]>,
): void {
  let left = keys.filter((key) => !pixels.has(key))
  while (left.length > 0) {
    const ring: Array<[number, readonly [number, number, number, number]]> = []
    const rest: number[] = []
    for (const key of left) {
      let red = 0
      let green = 0
      let blue = 0
      let count = 0
      for (const next of [key - 1, key + 1, key - width, key + width]) {
        const known = pixels.get(next)
        if (!known) continue
        red += known[0]
        green += known[1]
        blue += known[2]
        count += 1
      }
      if (count === 0) rest.push(key)
      else ring.push([key, [Math.round(red / count), Math.round(green / count), Math.round(blue / count), 255]])
    }
    if (ring.length === 0) break
    for (const [key, rgba] of ring) pixels.set(key, rgba)
    left = rest
  }
}
