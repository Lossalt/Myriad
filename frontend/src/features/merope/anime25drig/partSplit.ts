import type { Anime25DOwnTexture } from './renderer'
import type { Anime25DPart, Anime25DPartProfile, Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'

/**
 * A hanging piece drawn into a layer it shares (a pendant on the choker, a
 * charm on a garter, an earring in the hair), cut out to swing on its own,
 * as a rigger would give it a layer. The import's vision inventory says where
 * it is (partProfile); here its pixels are told from what is around it by
 * colour, moved to a layer of their own drawn just over the host, and the hole
 * they leave in the host is filled from the host around it. A piece that
 * cannot be told apart cleanly stays as it was.
 */
export interface SplitPart {
  /** The piece's own layer: over its host, with its own pixels. */
  source: Anime25DPlaybackLayer
  pixels: CroppedLayerPixels
  host: Anime25DPlaybackLayer
  /** The host's pixels with the piece taken out and filled. */
  hostPixels: CroppedLayerPixels
}

/** A piece bigger than this share of the canvas is not a hanging piece. */
const MAX_AREA_SHARE = 0.03
/** A pixel this far (sum of channel differences) from the colour around the piece is the piece. */
const PIECE_DISTANCE = 72
/** The piece must be between these shares of the opaque pixels in its box. */
const PIECE_SHARE = [0.02, 0.85] as const
/** Lines this thin (px either side) are outlines, not pieces. */
const OUTLINE = 2
/** A piece reaches up to within this share of its box of where it hangs from. */
const HANG_GAP = 0.08
/** A hanging piece is at least this tall for its width. */
const HANG_ASPECT = 0.8
/** The inventory's boxes are often off by this much of their size: look there too. */
const BOX_SHIFTS = [0, 0.25, -0.25, 0.5, -0.5, 0.75, -0.75]
/** Layers a piece may be drawn into. */
const HOST_ROLES = new Set(['front-hair', 'back-hair', 'headwear', 'neckwear', 'neck', 'topwear', 'bottomwear', 'legwear', 'footwear', 'handwear', 'objects'])
/** Layers that go with the head; a piece hung from the head is drawn in one of them, and only such a piece. */
const HEAD_ROLES = new Set(['front-hair', 'back-hair', 'headwear'])

const cutHosts = new WeakMap<Anime25DPlaybackLayer, string>()

/** The layer a piece's own layer was cut out of: what it hangs from. */
export function cutFrom(source: Anime25DPlaybackLayer): string | undefined {
  return cutHosts.get(source)
}

/** The name of a piece's own layer, which says which part of the inventory it is. */
export function splitLayerName(index: number, part: Readonly<Anime25DPart>): string {
  return `part:${index}:${part.name}`
}

/** The part a layer is, by its name: drawn alone in it, or cut out to it. */
export function partOfLayer(profile: Anime25DPartProfile | undefined, name: string): Anime25DPart | null {
  const cut = /^part:(\d+):/.exec(name)
  if (cut) return profile?.parts[Number(cut[1])] ?? null
  return profile?.parts.find((part) => part.layer === name) ?? null
}

/** Whether a part is a hanging piece to cut out of the layer it shares. */
export function cutsOut(part: Readonly<Anime25DPart>, canvas: { width: number; height: number }): boolean {
  const [x0, y0, x1, y1] = part.box
  return !part.separate && part.motion === 'dangle' && (x1 - x0) * (y1 - y0) <= MAX_AREA_SHARE * canvas.width * canvas.height
}

/** The layer a piece is drawn in: the one the inventory named, or the topmost one opaque at its middle. */
function hostOf(
  part: Readonly<Anime25DPart>,
  layers: readonly Anime25DPlaybackLayer[],
  read: (layer: Anime25DPlaybackLayer) => CroppedLayerPixels | null,
): Anime25DPlaybackLayer | null {
  if (part.layer) return layers.find((layer) => layer.name === part.layer && HOST_ROLES.has(layer.role)) ?? null
  const cx = (part.box[0] + part.box[2]) / 2
  const cy = (part.box[1] + part.box[3]) / 2
  for (let i = layers.length - 1; i >= 0; i--) {
    const layer = layers[i]
    if (!HOST_ROLES.has(layer.role) || layer.fade) continue
    if (cx < layer.x || cx >= layer.x + layer.w || cy < layer.y || cy >= layer.y + layer.h) continue
    const art = read(layer)
    if (!art) continue
    const px = Math.floor(((cx - layer.x) / layer.w) * art.width)
    const py = Math.floor(((cy - layer.y) / layer.h) * art.height)
    if (art.pixels[(py * art.width + px) * 4 + 3] > 128) return layer
  }
  return null
}

/**
 * Cuts one piece out of its host's pixels. `box` is in the host's pixels.
 * Returns the piece's pixels (around the box, at x, y) and the host's, filled; null when the
 * piece is not told apart from what is around it.
 */
export function cutPiece(
  art: Readonly<CroppedLayerPixels>,
  box: readonly [number, number, number, number],
  anchorY = box[1],
): { piece: CroppedLayerPixels; x: number; y: number; host: CroppedLayerPixels } | null {
  // The inventory's boxes are loose: look a little around it.
  const margin = Math.max(2, 0.1 * Math.max(box[2] - box[0], box[3] - box[1]))
  const [x0, y0, x1, y1] = [
    Math.max(0, Math.floor(box[0] - margin)), Math.max(0, Math.floor(box[1] - margin)),
    Math.min(art.width, Math.ceil(box[2] + margin)), Math.min(art.height, Math.ceil(box[3] + margin)),
  ]
  const w = x1 - x0
  const h = y1 - y0
  if (w < 3 || h < 3) return null
  const px = art.pixels
  const at = (x: number, y: number) => (y * art.width + x) * 4
  // The colour around the piece: the median of the opaque pixels on the box's rim.
  const rim: number[][] = [[], [], []]
  for (let y = y0; y < y1; y++) {
    for (let x = x0; x < x1; x++) {
      if (y > y0 + 1 && y < y1 - 2 && x > x0 + 1 && x < x1 - 2) continue
      const i = at(x, y)
      if (px[i + 3] < 128) continue
      for (let c = 0; c < 3; c++) rim[c].push(px[i + c])
    }
  }
  const median = (values: number[]) => values.sort((a, b) => a - b)[values.length >> 1] ?? 0
  const around = rim[0].length ? rim.map(median) : null
  const mask = new Uint8Array(w * h)
  let opaque = 0
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = at(x0 + x, y0 + y)
      if (px[i + 3] < 64) continue
      opaque += 1
      // What is above where it hangs from is what it hangs from.
      if (y0 + y < anchorY) continue
      // Hanging over nothing, all of it is the piece; over the host, what differs from it.
      const differs = !around || Math.abs(px[i] - around[0]) + Math.abs(px[i + 1] - around[1]) + Math.abs(px[i + 2] - around[2]) > PIECE_DISTANCE
      if (differs) mask[y * w + x] = 1
    }
  }
  // What differs and runs on out of the box, to the sides or below, is the
  // host's own: a frill, an outline, a stripe. Where the host's art ends, the
  // piece may end too. A thin outline the piece is drawn across does not
  // carry it off with it: what is that thin is set aside first, and the
  // piece then takes back its own edge.
  const core = morph(morph(mask, w, h, OUTLINE, false), w, h, OUTLINE, true)
  keepEnclosed(core, w, h, { left: x0 > 0, right: x1 < art.width, bottom: y1 < art.height })
  // One piece, hanging from where it hangs: what reaches up to there, and
  // not what merely lies near it.
  if (!keepHanging(core, w, h, anchorY - y0 + Math.max(3, HANG_GAP * (box[3] - box[1])))) return null
  const near = morph(core, w, h, OUTLINE + 1, true)
  let count = 0
  const bounds = [w, h, -1, -1]
  for (let k = 0; k < mask.length; k++) {
    mask[k] &= near[k]
    if (!mask[k]) continue
    count += 1
    const x = k % w
    const y = (k - x) / w
    bounds[0] = Math.min(bounds[0], x)
    bounds[1] = Math.min(bounds[1], y)
    bounds[2] = Math.max(bounds[2], x)
    bounds[3] = Math.max(bounds[3], y)
  }
  if (!opaque || count < PIECE_SHARE[0] * opaque || count > PIECE_SHARE[1] * opaque) return null
  // What hangs is drawn out by its weight; a flat run is part of the host.
  if (bounds[3] - bounds[1] < HANG_ASPECT * (bounds[2] - bounds[0])) return null
  // Its soft edge too: opaque pixels next to it.
  const grown = mask.slice()
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      if (mask[y * w + x] || px[at(x0 + x, y0 + y) + 3] < 16) continue
      if ((x > 0 && mask[y * w + x - 1]) || (x < w - 1 && mask[y * w + x + 1]) || (y > 0 && mask[(y - 1) * w + x]) || (y < h - 1 && mask[(y + 1) * w + x])) {
        grown[y * w + x] = 1
      }
    }
  }
  const piece = new Uint8ClampedArray(w * h * 4)
  const host = new Uint8ClampedArray(px)
  const known = new Uint8Array(w * h)
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = at(x0 + x, y0 + y)
      if (grown[y * w + x]) {
        piece.set(px.subarray(i, i + 4), (y * w + x) * 4)
      } else if (px[i + 3] >= 128) {
        known[y * w + x] = 1
      }
    }
  }
  // The hole is filled from the host around it, ring by ring; where nothing of
  // the host is around (the piece hung over nothing), it is left clear.
  for (let pass = 0; pass < w + h; pass++) {
    let filled = 0
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        if (!grown[y * w + x] || known[y * w + x]) continue
        const sum = [0, 0, 0]
        let n = 0
        for (const [dx, dy] of NEIGHBOURS) {
          const nx = x + dx
          const ny = y + dy
          if (nx < 0 || ny < 0 || nx >= w || ny >= h || known[ny * w + nx] !== 1) continue
          const j = at(x0 + nx, y0 + ny)
          for (let c = 0; c < 3; c++) sum[c] += host[j + c]
          n += 1
        }
        if (n < 2) continue
        const i = at(x0 + x, y0 + y)
        for (let c = 0; c < 3; c++) host[i + c] = sum[c] / n
        host[i + 3] = 255
        known[y * w + x] = 2
        filled += 1
      }
    }
    for (let k = 0; k < known.length; k++) {
      if (known[k] === 2) known[k] = 1
    }
    if (!filled) break
  }
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      if (grown[y * w + x] && !known[y * w + x]) host[at(x0 + x, y0 + y) + 3] = 0
    }
  }
  return { piece: { pixels: piece, width: w, height: h }, x: x0, y: y0, host: { pixels: host, width: art.width, height: art.height } }
}

const NEIGHBOURS = [[-1, 0], [1, 0], [0, -1], [0, 1]] as const

/**
 * Keeps, of a w×h mask, the biggest connected part that reaches up to row
 * `reach`; clears the rest. False when none does.
 */
function keepHanging(mask: Uint8Array, w: number, h: number, reach: number): boolean {
  const label = new Int32Array(mask.length)
  let best = 0
  let bestSize = 0
  let next = 0
  for (let start = 0; start < mask.length; start++) {
    if (!mask[start] || label[start]) continue
    next += 1
    label[start] = next
    const queue = [start]
    let size = 0
    let top = h
    while (queue.length) {
      const k = queue.pop()!
      const x = k % w
      const y = (k - x) / w
      size += 1
      top = Math.min(top, y)
      for (const [dx, dy] of NEIGHBOURS) {
        const nx = x + dx
        const ny = y + dy
        const n = ny * w + nx
        if (nx < 0 || ny < 0 || nx >= w || ny >= h || !mask[n] || label[n]) continue
        label[n] = next
        queue.push(n)
      }
    }
    if (top <= reach && size > bestSize) {
      best = next
      bestSize = size
    }
  }
  for (let k = 0; k < mask.length; k++) mask[k] = label[k] === best && best > 0 ? 1 : 0
  return best > 0
}

/** A w×h mask grown (or shrunk) by a square of radius r. */
function morph(mask: Uint8Array, w: number, h: number, r: number, grow: boolean): Uint8Array {
  // Separable: along rows, then along columns.
  const pass = (from: Uint8Array, along: 'x' | 'y') => {
    const to = new Uint8Array(from.length)
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        let hit = grow ? 0 : 1
        for (let d = -r; d <= r; d++) {
          const nx = along === 'x' ? x + d : x
          const ny = along === 'y' ? y + d : y
          const value = nx < 0 || ny < 0 || nx >= w || ny >= h ? 0 : from[ny * w + nx]
          if (grow ? value : !value) {
            hit = grow ? 1 : 0
            break
          }
        }
        to[y * w + x] = hit
      }
    }
    return to
  }
  return pass(pass(mask, 'x'), 'y')
}

/** Clears the parts of a w×h mask that reach the given edges; returns how much is left. */
function keepEnclosed(mask: Uint8Array, w: number, h: number, edges: { left: boolean; right: boolean; bottom: boolean }): number {
  const queue: number[] = []
  const clear = (k: number) => {
    if (mask[k] !== 1) return
    mask[k] = 0
    queue.push(k)
  }
  for (let x = 0; x < w && edges.bottom; x++) clear((h - 1) * w + x)
  for (let y = 0; y < h; y++) {
    if (edges.left) clear(y * w)
    if (edges.right) clear(y * w + w - 1)
  }
  while (queue.length) {
    const k = queue.pop()!
    const x = k % w
    const y = (k - x) / w
    for (const [dx, dy] of NEIGHBOURS) {
      const nx = x + dx
      const ny = y + dy
      if (nx >= 0 && ny >= 0 && nx < w && ny < h) clear(ny * w + nx)
    }
  }
  let count = 0
  for (const value of mask) count += value
  return count
}

/**
 * A part's piece where the inventory put it, or near there: its boxes are
 * often off by a good share of their size. The nearest box with a piece
 * hanging in it wins.
 */
function findPiece(art: Readonly<CroppedLayerPixels>, part: Readonly<Anime25DPart>, toX: (x: number) => number, toY: (y: number) => number) {
  const [x0, y0, x1, y1] = [toX(part.box[0]), toY(part.box[1]), toX(part.box[2]), toY(part.box[3])]
  const anchorY = toY(part.anchor[1])
  const shifts = BOX_SHIFTS.flatMap((dx) => BOX_SHIFTS.map((dy) => [dx, dy] as const))
    .sort((a, b) => Math.hypot(a[0], a[1]) - Math.hypot(b[0], b[1]))
  for (const [dx, dy] of shifts) {
    const ox = dx * (x1 - x0)
    const oy = dy * (y1 - y0)
    const result = cutPiece(art, [x0 + ox, y0 + oy, x1 + ox, y1 + oy], anchorY + oy)
    if (result) return result
  }
  return null
}

/** The hanging pieces of a package's inventory, cut out of the layers they share. */
export function splitHangingParts(
  layers: readonly Anime25DPlaybackLayer[],
  profile: Anime25DPartProfile | undefined,
  canvas: { width: number; height: number },
  read: (layer: Anime25DPlaybackLayer) => CroppedLayerPixels | null,
): SplitPart[] {
  const cut: SplitPart[] = []
  const hostPixels = new Map<Anime25DPlaybackLayer, CroppedLayerPixels>()
  profile?.parts.forEach((part, index) => {
    if (!cutsOut(part, canvas)) return
    const host = hostOf(part, layers, read)
    // An inventory that hangs it from the head but finds it in the clothes (or
    // the other way) has mistaken what it is: an "earring" on the collar is a brooch.
    if (!host || (part.bone === 'head') !== HEAD_ROLES.has(host.role)) return
    const art = hostPixels.get(host) ?? read(host)
    if (!art) return
    const sx = art.width / host.w
    const sy = art.height / host.h
    const result = findPiece(art, part, (x: number) => (x - host.x) * sx, (y: number) => (y - host.y) * sy)
    if (!result) return
    hostPixels.set(host, result.host)
    const { x: x0, y: y0 } = result
    const source: Anime25DPlaybackLayer = {
      ...host,
      name: splitLayerName(index, part),
      // Drawn just over its host.
      z: (host.z ?? layers.indexOf(host)) + 0.5,
      role: 'objects',
      side: null,
      phys: null,
      fade: null,
      x: host.x + x0 / sx,
      y: host.y + y0 / sy,
      w: result.piece.width / sx,
      h: result.piece.height / sy,
      atlas: {
        x: host.atlas.x + (x0 / art.width) * host.atlas.w,
        y: host.atlas.y + (y0 / art.height) * host.atlas.h,
        w: (result.piece.width / art.width) * host.atlas.w,
        h: (result.piece.height / art.height) * host.atlas.h,
      },
    }
    cutHosts.set(source, host.name)
    cut.push({ host, hostPixels: result.host, pixels: result.piece, source })
  })
  // A host cut twice keeps both cuts: each later piece reads it with the earlier ones out.
  for (const piece of cut) piece.hostPixels = hostPixels.get(piece.host) ?? piece.hostPixels
  return cut
}

/** A cut piece's pixels as a texture of its own, which its layer draws whole. */
export function createCutTexture(gl: WebGL2RenderingContext, image: Readonly<CroppedLayerPixels>): Anime25DOwnTexture | null {
  const texture = gl.createTexture()
  if (!texture) return null
  // Typed-array uploads are premultiplied here, as the atlas's patches are.
  const pixels = new Uint8Array(image.pixels.length)
  for (let i = 0; i < pixels.length; i += 4) {
    const alpha = image.pixels[i + 3]
    for (let c = 0; c < 3; c++) pixels[i + c] = Math.round((image.pixels[i + c] * alpha) / 255)
    pixels[i + 3] = alpha
  }
  gl.bindTexture(gl.TEXTURE_2D, texture)
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 0)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, image.width, image.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, pixels)
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 1)
  return { texture, uv: [0, 0, 1, 1] }
}
