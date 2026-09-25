import type {
  Anime25DSourceReference,
  RasterLayer,
} from './anime25dImportTypes'
import type {
  Anime25DPsdAnalysis,
  Anime25DPsdReconciliation,
} from './psdReconciliation'
import { anime25DLayerFade } from './anime25d'
import { uniquePartId } from './anime25dRaster'
import {
  analyzeAnime25DPsd,
  MATCH_DISTANCE,
  reportAnime25DPsdAnalysis,
  rgbDistance,
} from './psdReconciliation'

export interface Anime25DPsdRepair {
  layers: RasterLayer[]
  reconciliation: Anime25DPsdReconciliation | null
}

type Group = RasterLayer['group']

/**
 * A reveal grows past the verdict threshold to the covering art's real edge,
 * which keeps its own line art; this caps runaway growth in flat colour.
 */
const REVEAL_MAX_GROWTH = 4
const REVEAL_SOFT_EDGE = 2
const REVEAL_SOFT_EDGE_ALPHA = 160
/**
 * A covered layer that only matches pixel by pixel is not the same art: pale
 * skin under pale hair does. Faithful reveals measured 7-23; false ones 31-37.
 */
const REVEAL_MAX_MEAN_DISTANCE = 26
/** Bridges an accessory's thin parts, e.g. the bars of a birdcage earring. */
const RECOVER_CLOSING = 3
const RECOVER_MAX_HOLE_SHARE = 0.004
/** Larger recoveries would pin garment-sized static art onto moving layers. */
const RECOVER_MAX_SHARE = 0.02
const RECOVER_FRINGE = 1
const GROUP_VOTE_RING = 3

/**
 * Repairs what reconciliation can prove from the source illustration:
 * - `buried`: erase the covering pixels so the matching layer below shows,
 *   when that layer matches as a whole rather than by coincidence.
 * - `missing` / `mismatch`, and weak `buried`: lift the illustration's own
 *   pixels into a rigid `objects` layer per body group, drawn above whatever
 *   it overrides. Garment-sized areas are only reported.
 * Layers with expression variants are never edited or covered, so blinking and
 * speech stay live; such regions are only reported. Hidden surfaces cannot be
 * recovered because the illustration never shows them.
 */
export function repairAnime25DPsd(
  layers: readonly RasterLayer[],
  visibleAtRest: (layer: RasterLayer) => boolean,
  reference: Readonly<Anime25DSourceReference>,
  maxNewLayers: number,
): Anime25DPsdRepair {
  const visible = layers.filter(visibleAtRest)
  const before = analyzeAnime25DPsd(visible, reference)
  if (!before || before.status !== 'reconciled') {
    return {
      layers: [...layers],
      reconciliation: before && reportAnime25DPsdAnalysis(before, reference),
    }
  }
  const { bounds } = before
  const repaired = new Uint8Array(reference.width * reference.height)
  const copies = new Map<RasterLayer, RasterLayer>()
  const writable = (index: number) => {
    const original = visible[index]
    let copy = copies.get(original)
    if (!copy) {
      copy = { ...original, data: new Uint8ClampedArray(original.data) }
      copies.set(original, copy)
    }
    return copy
  }
  const at = (target: number) => ({
    x: bounds.x0 + (target % bounds.width),
    y: bounds.y0 + Math.floor(target / bounds.width),
  })

  const count = bounds.width * bounds.height
  // Specks below the size floor only help bridge a real region's fragments.
  const support = new Uint8Array(count)
  for (let target = 0; target < count; target += 1) {
    const code = before.flagged[target]
    if (code === 2 || code === 4) support[target] = 1
  }
  const seeds = new Uint8Array(count)
  for (const region of before.regions) {
    if (region.kind !== 'missing' && region.kind !== 'mismatch') continue
    for (const target of region.members) seeds[target] = 1
  }

  let revealed = 0
  for (const region of before.regions) {
    if (region.kind !== 'buried') continue
    const covering = (target: number) => {
      const { x, y } = at(target)
      const indices: number[] = []
      for (
        let index = before.covered[target] + 1;
        index < visible.length;
        index += 1
      ) {
        if (alphaAt(visible[index], x, y) > 0) indices.push(index)
      }
      return indices
    }
    if (
      region.members.some((target) =>
        covering(target).some((index) => hasExpressionVariants(visible[index])),
      )
    ) {
      continue
    }
    if (
      meanCoveredDistance(before, region.members, visible, reference) >
      REVEAL_MAX_MEAN_DISTANCE
    ) {
      for (const target of region.members) {
        seeds[target] = 1
        support[target] = 1
      }
      continue
    }
    const targets = new Map<number, number>()
    for (const target of region.members) {
      targets.set(target, before.covered[target])
    }
    // Grow while the covered art keeps beating the composite, so the reveal
    // ends at the covering drawing's own outline, not at a threshold contour.
    const queue = [...region.members]
    const limit = region.members.length * REVEAL_MAX_GROWTH
    while (queue.length > 0 && targets.size < limit) {
      const target = queue.pop()!
      const x = target % bounds.width
      const y = Math.floor(target / bounds.width)
      for (const next of [
        x > 0 ? target - 1 : -1,
        x < bounds.width - 1 ? target + 1 : -1,
        y > 0 ? target - bounds.width : -1,
        y < bounds.height - 1 ? target + bounds.width : -1,
      ]) {
        if (next < 0 || targets.has(next)) continue
        // A covering strand may span several layers underneath it.
        const matched = revealableBelow(
          before,
          visible,
          next,
          at(next),
          reference,
        )
        if (matched >= 0) {
          targets.set(next, matched)
          queue.push(next)
        }
      }
    }
    const erased = new Map<number, Array<{ x: number; y: number }>>()
    for (const [target, below] of targets) {
      const { x, y } = at(target)
      for (let index = below + 1; index < visible.length; index += 1) {
        const layer = visible[index]
        if (alphaAt(layer, x, y) === 0 || hasExpressionVariants(layer)) {
          continue
        }
        const copy = writable(index)
        copy.data[((y - copy.top) * copy.width + (x - copy.left)) * 4 + 3] = 0
        repaired[y * reference.width + x] = 1
        const points = erased.get(index) ?? []
        points.push({ x, y })
        erased.set(index, points)
      }
    }
    // The removed drawing's anti-aliased rim would linger as a ghost outline.
    for (const [index, points] of erased) {
      const copy = writable(index)
      for (const { x, y } of points) {
        for (let dy = -REVEAL_SOFT_EDGE; dy <= REVEAL_SOFT_EDGE; dy += 1) {
          for (let dx = -REVEAL_SOFT_EDGE; dx <= REVEAL_SOFT_EDGE; dx += 1) {
            const alpha = alphaAt(copy, x + dx, y + dy)
            if (alpha === 0 || alpha >= REVEAL_SOFT_EDGE_ALPHA) continue
            copy.data[
              ((y + dy - copy.top) * copy.width + (x + dx - copy.left)) * 4 + 3
            ] = 0
            repaired[(y + dy) * reference.width + x + dx] = 1
          }
        }
      }
    }
    revealed += 1
  }

  const pieces: RecoveredPiece[] = []
  {
    const mask = closeMask(
      support,
      bounds.width,
      bounds.height,
      RECOVER_CLOSING,
    )
    fillSmallHoles(
      mask,
      bounds.width,
      bounds.height,
      Math.max(64, Math.round(before.compared * RECOVER_MAX_HOLE_SHARE)),
    )
    const maxArea = Math.round(before.compared * RECOVER_MAX_SHARE)
    for (const members of components(mask, bounds.width, bounds.height)) {
      if (!members.some((target) => seeds[target])) continue
      const shown = members.filter((target) => {
        const { x, y } = at(target)
        return reference.data[(y * reference.width + x) * 4 + 3] >= 250
      })
      if (shown.length < before.minArea || shown.length > maxArea) continue
      if (
        shown.some((target) => {
          const { x, y } = at(target)
          return visible.some(
            (layer) =>
              hasExpressionVariants(layer) && alphaAt(layer, x, y) >= 128,
          )
        })
      ) {
        continue
      }
      const pixels = new Map<
        number,
        readonly [number, number, number, number]
      >()
      let above = -1
      for (const target of shown) {
        const { x, y } = at(target)
        const offset = (y * reference.width + x) * 4
        pixels.set(y * reference.width + x, [
          reference.data[offset],
          reference.data[offset + 1],
          reference.data[offset + 2],
          255,
        ])
        above = Math.max(above, before.top[target])
      }
      addAntialiasedRim(before, shown, reference, pixels)
      pieces.push({ pixels, above, ...mountFor(before, shown, visible) })
    }
  }
  // A piece of an existing accessory, e.g. the tassel under a hair ornament,
  // joins that drawing so both stay one rigid body. It only can when nothing
  // drawn above the accessory covers the piece at rest.
  let absorbed = 0
  const standalone: RecoveredPiece[] = []
  for (const piece of pieces) {
    if (piece.owner < 0 || piece.above > piece.owner) {
      standalone.push(piece)
      continue
    }
    const original = visible[piece.owner]
    copies.set(
      original,
      withPixels(
        copies.get(original) ?? original,
        piece.pixels,
        reference.width,
      ),
    )
    for (const key of piece.pixels.keys()) repaired[key] = 1
    absorbed += 1
  }
  // One layer per piece lets each follow what it hangs from; over budget,
  // pieces of one body part share a plain rigid layer instead.
  const recoveredLayers =
    standalone.length <= maxNewLayers
      ? standalone
      : mergeByGroup(standalone, maxNewLayers)
  for (const piece of recoveredLayers) {
    for (const key of piece.pixels.keys()) repaired[key] = 1
  }
  const recovered =
    absorbed +
    standalone.filter((piece) =>
      recoveredLayers.some(
        (layer) => layer === piece || layer.group === piece.group,
      ),
    ).length

  let output = layers.map((layer) => copies.get(layer) ?? layer)
  for (const piece of recoveredLayers) {
    const layer = overlayLayer(piece, reference.width, output)
    if (!layer) continue
    const anchor =
      piece.above >= 0
        ? visible[piece.above]
        : piece.neighbour >= 0
          ? visible[piece.neighbour]
          : visible.findLast((candidate) => candidate.group === piece.group)
    const index = anchor
      ? output.indexOf(copies.get(anchor) ?? anchor)
      : output.length - 1
    output = output.toSpliced(index + 1, 0, layer)
  }

  const after = analyzeAnime25DPsd(output.filter(visibleAtRest), reference)
  return {
    layers: output,
    reconciliation: after
      ? reportAnime25DPsdAnalysis(
          after,
          reference,
          { revealed, recovered },
          repaired,
        )
      : null,
  }
}

function hasExpressionVariants(layer: RasterLayer): boolean {
  return Boolean(layer.slot) || anime25DLayerFade(layer.role) !== null
}

function alphaAt(layer: RasterLayer, x: number, y: number): number {
  const localX = x - layer.left
  const localY = y - layer.top
  if (
    localX < 0 ||
    localY < 0 ||
    localX >= layer.width ||
    localY >= layer.height
  ) {
    return 0
  }
  return layer.data[(localY * layer.width + localX) * 4 + 3]
}

/** Neighbours within `radius` of a region, each paired with its nearest member. */
function fringe(
  members: readonly number[],
  width: number,
  height: number,
  radius: number,
): Map<number, number> {
  const ring = new Map<number, number>()
  for (const member of members) {
    const x = member % width
    const y = Math.floor(member / width)
    for (let dy = -radius; dy <= radius; dy += 1) {
      for (let dx = -radius; dx <= radius; dx += 1) {
        const nx = x + dx
        const ny = y + dy
        if (nx < 0 || ny < 0 || nx >= width || ny >= height) continue
        const next = ny * width + nx
        if (!ring.has(next)) ring.set(next, member)
      }
    }
  }
  return ring
}

/** Only erase fringe pixels where the covered art is the closer match. */
function revealsBetter(
  analysis: Readonly<Anime25DPsdAnalysis>,
  below: RasterLayer,
  target: number,
  { x, y }: { x: number; y: number },
  reference: Readonly<Anime25DSourceReference>,
): boolean {
  if (alphaAt(below, x, y) < 128) return false
  const referenceOffset = (y * reference.width + x) * 4
  if (reference.data[referenceOffset + 3] < 250) return false
  const belowOffset = ((y - below.top) * below.width + (x - below.left)) * 4
  const belowDistance = Math.hypot(
    below.data[belowOffset] - reference.data[referenceOffset],
    below.data[belowOffset + 1] - reference.data[referenceOffset + 1],
    below.data[belowOffset + 2] - reference.data[referenceOffset + 2],
  )
  if (belowDistance >= MATCH_DISTANCE) return false
  const compositeDistance = Math.hypot(
    analysis.color[target * 3] - reference.data[referenceOffset],
    analysis.color[target * 3 + 1] - reference.data[referenceOffset + 1],
    analysis.color[target * 3 + 2] - reference.data[referenceOffset + 2],
  )
  return compositeDistance > belowDistance + 10
}

/** The highest covered layer whose art beats the composite at a pixel. */
function revealableBelow(
  analysis: Readonly<Anime25DPsdAnalysis>,
  visible: readonly RasterLayer[],
  target: number,
  position: { x: number; y: number },
  reference: Readonly<Anime25DSourceReference>,
): number {
  for (let index = analysis.top[target] - 1; index >= 0; index -= 1) {
    if (revealsBetter(analysis, visible[index], target, position, reference)) {
      return index
    }
  }
  return -1
}

interface RecoveredPiece {
  pixels: Map<number, readonly [number, number, number, number]>
  /** Topmost layer the piece overrides, or -1 over open backdrop. */
  above: number
  /** The layer it touches most, used to place art over open backdrop. */
  neighbour: number
  /** A rigid accessory this piece belongs to, or -1. */
  owner: number
  group: Group
  role: RasterLayer['role']
}

const ACCESSORY_ROLES = new Set([
  'headwear',
  'earwear',
  'neckwear',
  'eyewear',
  'wings',
  'tail',
])
/** An accessory touching this much of the rim is taken as the piece's owner. */
const ACCESSORY_VOTE_SHARE = 0.15
/** Hanging art is held at its top: that end must reach what holds it. */
const HOOK_REACH = 8
const HOOK_BAND = 0.2

/**
 * Recovered art should move with what it hangs from: a tassel under a hair
 * ornament shares that ornament's role and so its mount and depth, an earring
 * mounts on the ears, anything on hair rides the hair.
 */
function mountFor(
  analysis: Readonly<Anime25DPsdAnalysis>,
  members: readonly number[],
  visible: readonly RasterLayer[],
): Pick<RecoveredPiece, 'group' | 'neighbour' | 'owner' | 'role'> {
  const votes = new Map<number, number>()
  let total = 0
  for (const [target] of fringe(
    members,
    analysis.bounds.width,
    analysis.bounds.height,
    GROUP_VOTE_RING,
  )) {
    const top = analysis.top[target]
    if (top < 0) continue
    votes.set(top, (votes.get(top) ?? 0) + 1)
    total += 1
  }
  if (total === 0) {
    return { group: 'body', neighbour: -1, owner: -1, role: 'objects' }
  }
  const ranked = [...votes].toSorted((left, right) => right[1] - left[1])
  const neighbour = ranked[0][0]
  const accessory = ranked.find(
    ([index, count]) =>
      ACCESSORY_ROLES.has(visible[index].role) &&
      count / total >= ACCESSORY_VOTE_SHARE,
  )
  const holder = hangsFrom(analysis, members, visible, (layer) =>
    ACCESSORY_ROLES.has(layer.role),
  )
  if (holder >= 0) {
    const owner = visible[holder]
    return {
      group: owner.group,
      neighbour: holder,
      owner: hasExpressionVariants(owner) ? -1 : holder,
      role: owner.role,
    }
  }
  if (accessory) {
    const owner = visible[accessory[0]]
    return {
      group: owner.group,
      neighbour: accessory[0],
      owner: hasExpressionVariants(owner) ? -1 : accessory[0],
      role: owner.role,
    }
  }
  if (
    hangsFrom(
      analysis,
      members,
      visible,
      (layer) => layer.role === 'ears' || layer.role === 'earwear',
    ) >= 0
  ) {
    return { group: 'head', neighbour, owner: -1, role: 'earwear' }
  }
  const main = visible[neighbour]
  if (main.role === 'front-hair' || main.role === 'back-hair') {
    return { group: 'head', neighbour, owner: -1, role: 'headwear' }
  }
  if (
    main.role === 'neck' ||
    main.role === 'collar-front' ||
    main.role === 'collar-back'
  ) {
    return { group: 'body', neighbour, owner: -1, role: 'neckwear' }
  }
  return { group: main.group, neighbour, owner: -1, role: 'objects' }
}

/** The first layer matching `accepts` that the piece's top end reaches. */
function hangsFrom(
  analysis: Readonly<Anime25DPsdAnalysis>,
  members: readonly number[],
  visible: readonly RasterLayer[],
  accepts: (layer: RasterLayer) => boolean,
): number {
  const candidates = visible
    .map((layer, index) => ({ layer, index }))
    .filter(({ layer }) => accepts(layer))
  if (candidates.length === 0) return -1
  const { bounds } = analysis
  let top = Number.POSITIVE_INFINITY
  let bottom = 0
  for (const target of members) {
    const y = Math.floor(target / bounds.width)
    top = Math.min(top, y)
    bottom = Math.max(bottom, y)
  }
  const band = top + Math.max(4, (bottom - top) * HOOK_BAND)
  for (const target of members) {
    if (Math.floor(target / bounds.width) > band) continue
    const x = bounds.x0 + (target % bounds.width)
    const y = bounds.y0 + Math.floor(target / bounds.width)
    for (let dy = -HOOK_REACH; dy <= HOOK_REACH; dy += 2) {
      for (let dx = -HOOK_REACH; dx <= HOOK_REACH; dx += 2) {
        const hook = candidates.find(
          ({ layer }) => alphaAt(layer, x + dx, y + dy) >= 128,
        )
        if (hook) return hook.index
      }
    }
  }
  return -1
}

function mergeByGroup(
  pieces: readonly RecoveredPiece[],
  maxLayers: number,
): RecoveredPiece[] {
  const merged = new Map<Group, RecoveredPiece>()
  for (const piece of pieces) {
    const existing = merged.get(piece.group)
    if (!existing) {
      if (merged.size >= maxLayers) continue
      merged.set(piece.group, {
        ...piece,
        pixels: new Map(piece.pixels),
        role: 'objects',
      })
      continue
    }
    for (const [key, rgba] of piece.pixels) existing.pixels.set(key, rgba)
    existing.above = Math.max(existing.above, piece.above)
    if (existing.neighbour < 0) existing.neighbour = piece.neighbour
  }
  return [...merged.values()]
}

/** Recovers anti-aliased edges of art that sits on the flat backdrop. */
function addAntialiasedRim(
  analysis: Readonly<Anime25DPsdAnalysis>,
  members: readonly number[],
  reference: Readonly<Anime25DSourceReference>,
  pixels: Map<number, readonly [number, number, number, number]>,
): void {
  const { bounds, background } = analysis
  if (!background.known) return
  for (const [target] of fringe(
    members,
    bounds.width,
    bounds.height,
    RECOVER_FRINGE,
  )) {
    if (analysis.alpha[target] >= 0.5) continue
    const x = bounds.x0 + (target % bounds.width)
    const y = bounds.y0 + Math.floor(target / bounds.width)
    const key = y * reference.width + x
    if (pixels.has(key)) continue
    const offset = key * 4
    if (reference.data[offset + 3] < 250) continue
    const coverage = Math.min(
      1,
      rgbDistance(reference.data, offset, background.color) / MATCH_DISTANCE,
    )
    if (coverage < 0.05) continue
    const unmix = (channel: number) =>
      Math.round(
        (reference.data[offset + channel] -
          background.color[channel] * (1 - coverage)) /
          coverage,
      )
    pixels.set(key, [unmix(0), unmix(1), unmix(2), Math.round(coverage * 255)])
  }
}

function meanCoveredDistance(
  analysis: Readonly<Anime25DPsdAnalysis>,
  members: readonly number[],
  visible: readonly RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
): number {
  const { bounds } = analysis
  let sum = 0
  for (const target of members) {
    const layer = visible[analysis.covered[target]]
    const x = bounds.x0 + (target % bounds.width)
    const y = bounds.y0 + Math.floor(target / bounds.width)
    const offset = ((y - layer.top) * layer.width + (x - layer.left)) * 4
    const referenceOffset = (y * reference.width + x) * 4
    sum += Math.hypot(
      layer.data[offset] - reference.data[referenceOffset],
      layer.data[offset + 1] - reference.data[referenceOffset + 1],
      layer.data[offset + 2] - reference.data[referenceOffset + 2],
    )
  }
  return members.length > 0 ? sum / members.length : 0
}

/** Square-kernel morphological closing, separable per axis. */
function closeMask(
  mask: Uint8Array,
  width: number,
  height: number,
  radius: number,
): Uint8Array {
  const dilated = sweep(
    sweep(mask, width, height, radius, 1, true),
    width,
    height,
    radius,
    width,
    true,
  )
  return sweep(
    sweep(dilated, width, height, radius, 1, false),
    width,
    height,
    radius,
    width,
    false,
  )
}

/** One axis of a dilation (`grow`) or erosion; `step` 1 is x, `width` is y. */
function sweep(
  mask: Uint8Array,
  width: number,
  height: number,
  radius: number,
  step: number,
  grow: boolean,
): Uint8Array {
  const output = new Uint8Array(mask.length)
  const length = step === 1 ? width : height
  const lines = step === 1 ? height : width
  for (let line = 0; line < lines; line += 1) {
    const start = step === 1 ? line * width : line
    for (let position = 0; position < length; position += 1) {
      let value = grow ? 0 : 1
      for (let offset = -radius; offset <= radius; offset += 1) {
        const other = position + offset
        // Outside counts as set when eroding so edges are not eaten away.
        const sample =
          other < 0 || other >= length
            ? grow
              ? 0
              : 1
            : mask[start + other * step]
        if (grow ? sample : !sample) {
          value = grow ? 1 : 0
          break
        }
      }
      output[start + position * step] = value
    }
  }
  return output
}

/** Fills enclosed gaps, e.g. hair seen through a birdcage, up to `maxArea`. */
function fillSmallHoles(
  mask: Uint8Array,
  width: number,
  height: number,
  maxArea: number,
): void {
  const inverse = new Uint8Array(mask.length)
  for (let index = 0; index < mask.length; index += 1) {
    inverse[index] = mask[index] ? 0 : 1
  }
  for (const hole of components(inverse, width, height)) {
    if (hole.length > maxArea) continue
    if (
      hole.some((target) => {
        const x = target % width
        const y = Math.floor(target / width)
        return x === 0 || y === 0 || x === width - 1 || y === height - 1
      })
    ) {
      continue
    }
    for (const target of hole) mask[target] = 1
  }
}

function components(
  mask: Uint8Array,
  width: number,
  height: number,
): number[][] {
  const visited = new Uint8Array(mask.length)
  const result: number[][] = []
  const stack: number[] = []
  for (let start = 0; start < mask.length; start += 1) {
    if (!mask[start] || visited[start]) continue
    const members: number[] = []
    visited[start] = 1
    stack.push(start)
    while (stack.length > 0) {
      const target = stack.pop()!
      members.push(target)
      const x = target % width
      const y = Math.floor(target / width)
      for (const next of [
        x > 0 ? target - 1 : -1,
        x < width - 1 ? target + 1 : -1,
        y > 0 ? target - width : -1,
        y < height - 1 ? target + width : -1,
      ]) {
        if (next < 0 || visited[next] || !mask[next]) continue
        visited[next] = 1
        stack.push(next)
      }
    }
    result.push(members)
  }
  return result
}

/** Paints recovered pixels into a layer, growing its bounds as needed. */
function withPixels(
  layer: RasterLayer,
  pixels: ReadonlyMap<number, readonly [number, number, number, number]>,
  referenceWidth: number,
): RasterLayer {
  let left = layer.left
  let top = layer.top
  let right = layer.left + layer.width
  let bottom = layer.top + layer.height
  for (const key of pixels.keys()) {
    const x = key % referenceWidth
    const y = Math.floor(key / referenceWidth)
    left = Math.min(left, x)
    top = Math.min(top, y)
    right = Math.max(right, x + 1)
    bottom = Math.max(bottom, y + 1)
  }
  const width = right - left
  const height = bottom - top
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < layer.height; y += 1) {
    const start = y * layer.width * 4
    data.set(
      layer.data.subarray(start, start + layer.width * 4),
      ((y + layer.top - top) * width + layer.left - left) * 4,
    )
  }
  for (const [key, [red, green, blue, alpha]] of pixels) {
    const offset =
      ((Math.floor(key / referenceWidth) - top) * width +
        (key % referenceWidth) -
        left) *
      4
    // Over, in straight alpha: the source illustration wins where it is opaque.
    const coverage = alpha / 255
    const below = data[offset + 3] / 255
    const combined = coverage + below * (1 - coverage)
    if (combined <= 0) continue
    const mix = (channel: number, value: number) =>
      (value * coverage + data[offset + channel] * below * (1 - coverage)) /
      combined
    data[offset] = mix(0, red)
    data[offset + 1] = mix(1, green)
    data[offset + 2] = mix(2, blue)
    data[offset + 3] = combined * 255
  }
  return { ...layer, left, top, width, height, data }
}

function overlayLayer(
  { group, pixels, role }: RecoveredPiece,
  referenceWidth: number,
  layers: readonly RasterLayer[],
): RasterLayer | null {
  if (pixels.size === 0) return null
  let left = Number.POSITIVE_INFINITY
  let top = Number.POSITIVE_INFINITY
  let right = 0
  let bottom = 0
  for (const key of pixels.keys()) {
    const x = key % referenceWidth
    const y = Math.floor(key / referenceWidth)
    left = Math.min(left, x)
    top = Math.min(top, y)
    right = Math.max(right, x + 1)
    bottom = Math.max(bottom, y + 1)
  }
  const width = right - left
  const height = bottom - top
  const data = new Uint8ClampedArray(width * height * 4)
  for (const [key, rgba] of pixels) {
    const x = (key % referenceWidth) - left
    const y = Math.floor(key / referenceWidth) - top
    data.set(rgba, (y * width + x) * 4)
  }
  const id = uniquePartId(
    `recovered-${role}`,
    new Set(layers.map((layer) => layer.id)),
  )
  return {
    id,
    role,
    sourceName: id,
    order: 0,
    side: null,
    group,
    left,
    top,
    width,
    height,
    data,
    synthetic: true,
  }
}
