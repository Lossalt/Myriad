/** 选中卡与文章轨同一左缘；前一张从左边伸出，不退场藏掉。 */

import type { FeedStory } from '../logic/feedStories'
import type { TimeTranslations } from '../types'

import type { StoryCardFace } from './storyFace'
import {
  clampConversationScroll,
  CONVERSATION_FADE_PX,
  decayVelocity,
} from '../../agent-panel/conversationPan'
import {
  cloneStoryCardInner,
  storyCardFace,

  storyCardInnerHtml,
} from './storyFace'

/** 比对话轨略慢，才能看见下一张进来。 */
const RAIL_FOLLOW_TAU = 0.04

const RAIL_SNAP_FAR_TAU = 0.046
const RAIL_SNAP_MID_TAU = 0.06
const RAIL_SNAP_NEAR_TAU = 0.072

const RAIL_CRUISE_SNAP_FAR_TAU = 0.14
const RAIL_CRUISE_SNAP_MID_TAU = 0.18
const RAIL_CRUISE_SNAP_NEAR_TAU = 0.22

/** 坐进槽位时提前咬死，去掉指数衰减的长尾巴。 */
export const RAIL_SEAT_PX = 2.6

export const RAIL_WHEEL_SETTLE_MS = 120

/** 甩手惯性比对话轨长，宽卡才滑得开。 */
export const RAIL_FLING_TAU = 0.38

/** 低于这个速度就停，别卡在对话轨的 90px/s 突然钉死。 */
export const RAIL_FLING_MIN_PX_S = 24

/** 点选和拖轨的分界。 */
export const RAIL_DRAG_SLOP_PX = 5

export const RAIL_FLING_SLOT_PX_S = 360

export const RAIL_OVERFLOW_LEFT_PX = 0

/** 左边越出的卡保持绘制。 */
export function railOverflowLeft(left: number, enabled: boolean): boolean {
  return enabled && left < CONVERSATION_FADE_PX
}

/** 左缘溢出留下；进画和右侧预显示都不退场。 */
export function railCardKeepsPaint(
  left: number,
  overflowLeft: boolean,
  fade = CONVERSATION_FADE_PX,
): boolean {
  return left >= fade || railOverflowLeft(left, overflowLeft)
}

export function railLeadColumn(scroll: number, colW: number): number {
  return Math.floor(Math.max(0, scroll) / Math.max(1, colW)) + 1
}

export function railTrackScroll(
  track: { dataset: { phantasiRailScroll?: string }; style: { transform: string } } | null,
  fallback = 0,
): number {
  if (!track) return fallback
  const raw = track.style.transform
  if (raw && raw !== 'none') {
    return Math.max(0, scrollFromTrackTransform(raw))
  }
  const stored = Number(track.dataset.phantasiRailScroll)
  if (Number.isFinite(stored)) return Math.max(0, stored)
  return fallback
}

/** 只实装视口里的列，右边多留几列做预显示。 */
export function railMountColumns(
  scroll: number,
  viewW: number,
  colW: number,
  totalCols: number,
  leftPad = 1,
  rightPad = 3,
): { from: number; to: number } {
  const cols = Math.max(1, totalCols)
  const width = Math.max(1, colW)
  const start = Math.floor(Math.max(0, scroll) / width) + 1
  const visible = Math.max(1, Math.ceil(Math.max(viewW, width) / width))
  return {
    from: Math.max(1, start - leftPad),
    to: Math.min(cols, start + visible + rightPad - 1),
  }
}

/** 预热带往前挪时只要新列，已热过的列不再扫。 */
export function eagerStoryDelta(
  from: number,
  to: number,
  prevFrom: number,
  prevTo: number,
): Array<{ from: number; to: number }> {
  if (to < from) return []
  if (prevTo < prevFrom) return [{ from, to }]
  if (to < prevFrom || from > prevTo) return [{ from, to }]
  const ranges: Array<{ from: number; to: number }> = []
  if (from < prevFrom) ranges.push({ from, to: Math.min(to, prevFrom - 1) })
  if (to > prevTo) ranges.push({ from: Math.max(from, prevTo + 1), to })
  return ranges
}

function eagerStorySrc(img: HTMLImageElement | null): void {
  if (!img || typeof img.getAttribute !== 'function') return
  const src = img.getAttribute('data-src')
  if (src && img.getAttribute('src') !== src) img.src = src
}

export function onStoryMediaError(event: Event): void {
  const img = event.target
  if (!(img instanceof Element)) return
  if (img.closest('.phantasi-story__source')) {
    onHeldIconError(event)
    return
  }
  if (img.closest('.phantasi-story__thumb')) onHeldCoverError(event)
}

function onHeldCoverError(event: Event): void {
  const img = event.target
  if (!(img instanceof Element)) return
  const thumb = img.closest('.phantasi-story__thumb')
  const story = img.closest('.phantasi-story')
  if (thumb instanceof HTMLElement) thumb.hidden = true
  story?.classList.remove('has-cover')
}

function onHeldIconError(event: Event): void {
  const img = event.target
  if (img instanceof HTMLElement) img.hidden = true
}

function mountHeldStoryImg(
  host: HTMLElement | null,
  kind: 'cover' | 'icon',
): HTMLImageElement | null {
  if (!host || typeof host.getAttribute !== 'function') return null
  const src = host.getAttribute('data-src')
  if (!src || typeof document === 'undefined') return null
  const img = document.createElement('img')
  img.alt = ''
  img.decoding = 'async'
  img.loading = 'eager'
  img.setAttribute('data-src', src)
  img.src = src
  img.addEventListener(
    'error',
    kind === 'cover' ? onHeldCoverError : onHeldIconError,
  )
  host.appendChild(img)
  return img
}

const eagerStories = new WeakSet<object>()

function storyPart(
  story: {
    children?: HTMLCollection
    querySelector: ParentNode['querySelector']
  },
  className: string,
): HTMLElement | null {
  const kids = story.children
  if (kids && kids.length > 0) {
    for (let i = 0; i < kids.length; i++) {
      const el = kids[i] as HTMLElement
      if (el.classList?.contains(className)) return el
      const nested = el.getElementsByClassName?.(className)[0]
      if (nested) return nested as HTMLElement
    }
    return null
  }
  return story.querySelector(`.${className}`) as HTMLElement | null
}

function eagerStoryCover(story: {
  querySelector: ParentNode['querySelector']
  children?: HTMLCollection
  classList?: DOMTokenList
}): void {
  if (story.classList?.contains('phantasi-story--slot')) return
  if (eagerStories.has(story)) {
    story.classList?.remove('is-hold')
    return
  }
  story.classList?.remove('is-hold')
  const thumb = storyPart(story, 'phantasi-story__thumb')
  let img = thumb?.querySelector?.('img') as HTMLImageElement | null
  if (!img) {
    img = story.querySelector('.phantasi-story__thumb img') as HTMLImageElement | null
  }
  if (!img) img = mountHeldStoryImg(thumb, 'cover')
  if (img && img.loading !== 'eager') img.loading = 'eager'
  eagerStorySrc(img)
  const source = storyPart(story, 'phantasi-story__source')
  let icon = source?.querySelector?.('img') as HTMLImageElement | null
  if (!icon) {
    icon = story.querySelector('.phantasi-story__source img') as HTMLImageElement | null
  }
  if (!icon) icon = mountHeldStoryImg(source, 'icon')
  eagerStorySrc(icon)
  eagerStories.add(story)
}

const storyColCache = new WeakMap<
  ParentNode,
  { byCol: Map<number, HTMLElement[]> }
>()
const wantStoryCols = new Set<number>()
const awayDeltaCols: number[] = []
const eagerDeltaCols: number[] = []

function liveStoryEls(list: HTMLElement[] | undefined): HTMLElement[] {
  if (!list || list.length === 0) return []
  let write = 0
  for (let i = 0; i < list.length; i++) {
    const el = list[i]
    if (!el || el.isConnected === false) continue
    if (write !== i) list[write] = el
    write += 1
  }
  list.length = write
  return list
}

function collectStoryCols(
  kids: HTMLCollection,
  byCol: Map<number, HTMLElement[]>,
  want: ReadonlySet<number>,
): void {
  if (want.size === 0) return
  let min = Infinity
  for (const col of want) {
    if (col < min) min = col
  }
  for (let i = kids.length - 1; i >= 0; i--) {
    const story = kids[i] as HTMLElement
    const col = Number(story.dataset?.railCol)
    if (!Number.isFinite(col) || col < 1) continue
    if (col < min) break
    if (!want.has(col)) continue
    const list = byCol.get(col)
    if (list) list.push(story)
    else byCol.set(col, [story])
  }
  for (const col of want) {
    if ((byCol.get(col)?.length ?? 0) > 0) continue
    for (const missed of want) byCol.delete(missed)
    for (let i = 0; i < kids.length; i++) {
      const story = kids[i] as HTMLElement
      const next = Number(story.dataset?.railCol)
      if (!want.has(next)) continue
      const list = byCol.get(next)
      if (list) list.push(story)
      else byCol.set(next, [story])
    }
    return
  }
}

function indexStoryCols(
  track: ParentNode,
  cols?: ReadonlyArray<number>,
): Map<number, HTMLElement[]> | null {
  const kids = 'children' in track ? (track as Element).children : null
  if (!kids) return null
  let hit = storyColCache.get(track)
  if (!hit) {
    hit = { byCol: new Map() }
    storyColCache.set(track, hit)
  }
  const { byCol } = hit
  if (!cols) {
    if (byCol.size === 0) {
      for (let i = 0; i < kids.length; i++) {
        const story = kids[i] as HTMLElement
        const col = Number(story.dataset?.railCol)
        if (!Number.isFinite(col) || col < 1) continue
        const list = byCol.get(col)
        if (list) list.push(story)
        else byCol.set(col, [story])
      }
    }
    return byCol
  }
  wantStoryCols.clear()
  for (const col of cols) {
    if (liveStoryEls(byCol.get(col)).length === 0) wantStoryCols.add(col)
  }
  collectStoryCols(kids, byCol, wantStoryCols)
  return byCol
}

function eachStoryInRanges(
  byCol: Map<number, HTMLElement[]>,
  ranges: ReadonlyArray<{ from: number; to: number }>,
  visit: (story: HTMLElement) => void,
): void {
  for (const range of ranges) {
    for (let col = range.from; col <= range.to; col++) {
      const stories = liveStoryEls(byCol.get(col))
      if (stories.length === 0) continue
      for (const story of stories) visit(story)
    }
  }
}

/** 只把视口和右侧预显的封面改成 eager，扩窗多挂的空列不解码。 */
export function eagerStoryCovers(
  track: ParentNode | null,
  from: number,
  to: number,
  prev?: { from: number; to: number },
): void {
  if (!track || to < from) return
  const ranges = prev
    ? eagerStoryDelta(from, to, prev.from, prev.to)
    : [{ from, to }]
  if (ranges.length === 0) return
  eagerDeltaCols.length = 0
  for (const range of ranges) {
    for (let col = range.from; col <= range.to; col++) eagerDeltaCols.push(col)
  }
  const byCol = indexStoryCols(track, eagerDeltaCols)
  if (byCol) {
    eachStoryInRanges(byCol, ranges, eagerStoryCover)
    return
  }
  for (const range of ranges) {
    for (let col = range.from; col <= range.to; col++) {
      for (const story of track.querySelectorAll<HTMLElement>(
        `[data-rail-col="${col}"]`,
      )) {
        eagerStoryCover(story)
      }
    }
  }
}

function toggleStoryAway(story: { classList?: DOMTokenList }, away: boolean): void {
  story.classList?.toggle('is-away', away)
  if (!away) story.classList?.remove('is-hold')
}

const livePaintCols: number[] = []

function paintStoryShell(
  el: HTMLElement,
  face: StoryCardFace,
  html: string,
  hold: boolean,
  showStar: boolean,
  deferCover: boolean,
): void {
  const peeking = el.classList.contains('is-peek')
  el.className =
    `phantasi-story phantasi-float phantasi-story__hit phantasi-story__shell${
      face.unread ? ' is-unread' : ''
    }${face.cover ? ' has-cover' : ''}${showStar ? ' has-star' : ''}${
      hold ? ' is-hold' : ''
    }${peeking ? ' is-peek' : ''}`
  el.dataset.railId = String(face.id)
  el.removeAttribute('aria-hidden')
  el.removeAttribute('tabindex')
  if (face.hue) el.style.setProperty('--story-topic', face.hue)
  else el.style.removeProperty?.('--story-topic')
  const node = cloneStoryCardInner(face, (deferCover ? 1 : 0) | (showStar ? 2 : 0))
  if (node && typeof el.replaceChildren === 'function') {
    el.replaceChildren(node)
    return
  }
  el.innerHTML = html
}

function storyShellStyle(col: number, row: 1 | 2): string {
  const left = `calc(${col - 1} * (var(--phantasi-story-w) + 0.75rem))`
  const top = row === 2
    ? 'calc(var(--phantasi-story-h) + var(--phantasi-items-gap))'
    : '0px'
  return `position:absolute;left:${left};top:${top};width:var(--phantasi-story-w);height:var(--phantasi-story-h)`
}

const SHELL_POOL_CAP = 48
const shellPool: HTMLElement[] = []
const grabShellsByTrack = new WeakMap<ParentNode, HTMLElement[]>()

function recycleStoryShell(el: HTMLElement): void {
  if (shellPool.length >= SHELL_POOL_CAP) return
  eagerStories.delete(el)
  el.innerHTML = ''
  delete el.dataset.railId
  shellPool.push(el)
}

function rememberGrabShell(track: ParentNode, el: HTMLElement): void {
  let list = grabShellsByTrack.get(track)
  if (!list) {
    list = []
    grabShellsByTrack.set(track, list)
  }
  list.push(el)
}

function makeStoryShell(
  track: ParentNode,
  col: number,
  row: 1 | 2,
): HTMLElement | null {
  if (typeof document === 'undefined') return null
  const el = shellPool.pop() ?? document.createElement('button')
  el.setAttribute('type', 'button')
  el.className = 'phantasi-story phantasi-story--slot'
  el.dataset.railCol = String(col)
  el.dataset.phantasiDomShell = '1'
  el.setAttribute('aria-hidden', 'true')
  el.tabIndex = -1
  el.style.cssText = storyShellStyle(col, row)
  rememberGrabShell(track, el)
  return el
}

/** 手势扩窗不走 React 挂壳，只给预显垫补宿主。 */
export function ensureStoryShells(
  track: ParentNode | null,
  from: number,
  to: number,
  slots?: ReadonlyArray<{ column: number; row: 1 | 2 }>,
): Map<number, HTMLElement[]> | null {
  if (!track || to < from || typeof document === 'undefined') return null
  if (!('appendChild' in track)) return null
  livePaintCols.length = 0
  for (let col = from; col <= to; col++) livePaintCols.push(col)
  const byEl = indexStoryCols(track, livePaintCols)
  if (!byEl) return null
  const wantRows = new Map<number, Array<1 | 2>>()
  if (slots) {
    for (const slot of slots) {
      if (slot.column < from || slot.column > to) continue
      const rows = wantRows.get(slot.column)
      if (!rows) {
        wantRows.set(slot.column, [slot.row])
        continue
      }
      if (!rows.includes(slot.row)) rows.push(slot.row)
    }
  }
  const host = track as Element
  const frag =
    typeof document.createDocumentFragment === 'function'
      ? document.createDocumentFragment()
      : null
  for (let col = from; col <= to; col++) {
    const rows = wantRows.get(col)
    if (!rows) continue
    const stories = liveStoryEls(byEl.get(col))
    for (const row of rows) {
      if (stories[row - 1]) continue
      const el = makeStoryShell(track, col, row)
      if (!el) continue
      if (frag) frag.appendChild(el)
      else host.appendChild(el)
      stories.push(el)
      const list = byEl.get(col)
      if (list !== stories) {
        if (list) list.push(el)
        else byEl.set(col, stories)
      }
    }
  }
  if (frag && frag.childNodes.length > 0) host.appendChild(frag)
  return byEl
}

function recycleListedShell(
  el: HTMLElement,
  from: number,
  to: number,
  byCol?: Map<number, HTMLElement[]>,
): boolean {
  const railCol = Number(el.dataset?.railCol)
  if (!Number.isFinite(railCol) || (railCol >= from && railCol <= to)) {
    return false
  }
  el.remove()
  recycleStoryShell(el)
  byCol?.delete(railCol)
  return true
}

/** 手势里只留预显带上的壳，带外回收给下一列用。不拆 React 已挂列。 */
export function recycleStoryDomShellsOutside(
  track: ParentNode | null,
  from: number,
  to: number,
): void {
  if (!track || to < from) return
  const byCol = storyColCache.get(track)?.byCol
  const listed = grabShellsByTrack.get(track)
  if (listed && listed.length > 0) {
    let write = 0
    for (const el of listed) {
      if (recycleListedShell(el, from, to, byCol)) continue
      listed[write] = el
      write += 1
    }
    listed.length = write
    return
  }
  const kids = 'children' in track ? (track as Element).children : null
  if (!kids) return
  for (let i = kids.length - 1; i >= 0; i--) {
    const el = kids[i] as HTMLElement
    if (!el.dataset?.phantasiDomShell) continue
    recycleListedShell(el, from, to, byCol)
  }
}

/** 松手后拆掉手势补的壳，交给 React 接管。 */
export function dropStoryDomShells(track: ParentNode | null): void {
  if (track) storyColCache.delete(track)
  const listed = track ? grabShellsByTrack.get(track) : undefined
  if (listed && listed.length > 0) {
    for (const el of listed) {
      el.remove()
      recycleStoryShell(el)
    }
    listed.length = 0
    return
  }
  const kids = track && 'children' in track ? (track as Element).children : null
  if (!kids) return
  for (let i = kids.length - 1; i >= 0; i--) {
    const el = kids[i] as HTMLElement
    if (!el.dataset?.phantasiDomShell) continue
    el.remove()
    recycleStoryShell(el)
  }
}

interface LiveSlot { story: FeedStory; column: number; row: 1 | 2 }

function paintLiveSlot(
  stories: HTMLElement[],
  slot: LiveSlot,
  hold: boolean,
  forceDefer: boolean,
  times: TimeTranslations,
  locale: string,
  labels: {
    unread: string
    starred: string
    unstar: string
  },
  canStar: boolean,
): void {
  const el = stories[slot.row - 1]
  if (!el?.classList?.contains('phantasi-story--slot')) return
  const face = storyCardFace(slot.story, times, locale, labels)
  const deferCover = forceDefer || hold
  paintStoryShell(
    el,
    face,
    storyCardInnerHtml(
      face,
      labels.unread,
      labels.starred,
      labels.unstar,
      deferCover,
      canStar,
    ),
    hold,
    canStar,
    deferCover,
  )
}

/** 手势里把已挂的壳就地写成实卡，不走 React setLive。 */
export function paintStoryLiveCols(
  track: ParentNode | null,
  from: number,
  to: number,
  slots: ReadonlyArray<LiveSlot> | ReadonlyMap<number, readonly LiveSlot[]>,
  times: TimeTranslations,
  locale: string,
  labels: {
    unread: string
    starred: string
    unstar: string
  },
  holdAt: number,
  canStar: boolean,
  forceDefer = false,
  ready?: Map<number, HTMLElement[]> | null,
): void {
  if (!track || to < from) return
  let byEl = ready ?? null
  if (!byEl) {
    livePaintCols.length = 0
    for (let col = from; col <= to; col++) livePaintCols.push(col)
    byEl = indexStoryCols(track, livePaintCols)
  }
  if (!byEl) return
  if ('get' in slots) {
    for (let col = from; col <= to; col++) {
      const colSlots = slots.get(col)
      if (!colSlots || colSlots.length === 0) continue
      const stories = liveStoryEls(byEl.get(col))
      const hold = col > holdAt
      for (const slot of colSlots) {
        paintLiveSlot(stories, slot, hold, forceDefer, times, locale, labels, canStar)
      }
    }
    return
  }
  let stories: HTMLElement[] = []
  let storiesCol = 0
  for (const slot of slots) {
    const col = slot.column
    if (col < from || col > to) continue
    if (storiesCol !== col) {
      stories = liveStoryEls(byEl.get(col))
      storiesCol = col
    }
    paintLiveSlot(
      stories,
      slot,
      col > holdAt,
      forceDefer,
      times,
      locale,
      labels,
      canStar,
    )
  }
}

/** 视口和右侧预显保绘；左边已挂列只标 is-away，不拆卡。 */
export function paintStoryAway(
  track: ParentNode | null,
  from: number,
  to: number,
  prev?: { from: number; to: number },
  hide = true,
): void {
  if (!track || to < from) return
  if (prev && prev.from === from && prev.to === to) return
  awayDeltaCols.length = 0
  if (prev) {
    if (hide) {
      for (let col = prev.from; col <= prev.to; col++) {
        if (col < from || col > to) awayDeltaCols.push(col)
      }
    }
    for (let col = from; col <= to; col++) {
      if (col < prev.from || col > prev.to) awayDeltaCols.push(col)
    }
  }
  const byCol = prev ? indexStoryCols(track, awayDeltaCols) : null
  if (byCol && prev) {
    if (hide) {
      for (let col = prev.from; col <= prev.to; col++) {
        if (col >= from && col <= to) continue
        const stories = liveStoryEls(byCol.get(col))
        for (const story of stories) toggleStoryAway(story, true)
      }
    }
    for (let col = from; col <= to; col++) {
      if (col >= prev.from && col <= prev.to) continue
      const stories = liveStoryEls(byCol.get(col))
      for (const story of stories) toggleStoryAway(story, false)
    }
    return
  }
  const kids = 'children' in track ? (track as Element).children : null
  if (kids) {
    for (let i = 0; i < kids.length; i++) {
      const story = kids[i] as HTMLElement
      const col = Number(story.dataset?.railCol)
      if (!Number.isFinite(col) || col < 1) continue
      const away = col < from || col > to
      if (away && !hide) continue
      toggleStoryAway(story, away)
    }
    return
  }
  if (prev) {
    if (hide) {
      for (let col = prev.from; col <= prev.to; col++) {
        if (col >= from && col <= to) continue
        for (const story of track.querySelectorAll<HTMLElement>(
          `[data-rail-col="${col}"]`,
        )) {
          toggleStoryAway(story, true)
        }
      }
    }
    for (let col = from; col <= to; col++) {
      if (col >= prev.from && col <= prev.to) continue
      for (const story of track.querySelectorAll<HTMLElement>(
        `[data-rail-col="${col}"]`,
      )) {
        toggleStoryAway(story, false)
      }
    }
    return
  }
  for (const story of track.querySelectorAll<HTMLElement>('.phantasi-story')) {
    const col = Number(story.dataset?.railCol)
    if (!Number.isFinite(col) || col < 1) continue
    const away = col < from || col > to
    if (away && !hide) continue
    toggleStoryAway(story, away)
  }
}

/** 窗口还盖得住视口和右侧预显就别换，少在滚动里拆卡。 */
export function railMountColumnsSticky(
  prev: { from: number; to: number },
  scroll: number,
  viewW: number,
  colW: number,
  totalCols: number,
  leftPad = 1,
  rightPad = 3,
  slack = 2,
): { from: number; to: number } {
  const ideal = railMountColumns(scroll, viewW, colW, totalCols, leftPad, rightPad)
  const cols = Math.max(1, totalCols)
  if (
    prev.from >= 1
    && prev.to <= cols
    && prev.from <= ideal.from
    && prev.to >= ideal.to
  ) {
    return prev
  }
  return {
    from: Math.max(1, ideal.from - slack),
    to: Math.min(cols, ideal.to + slack),
  }
}

/** 停稳后仍多留的列，下次开滑少拆卡。 */
export const RAIL_MOUNT_SETTLE_EXTRA = 16

/** 停稳时左边只收到预显带外，不拆到视口边；过大才收回右边。 */
export function railMountColumnsSettle(
  prev: { from: number; to: number },
  scroll: number,
  viewW: number,
  colW: number,
  totalCols: number,
  leftPad = 1,
  rightPad = 3,
  slack = 2,
  keepExtra = RAIL_MOUNT_SETTLE_EXTRA,
): { from: number; to: number } {
  const need = railMountColumns(scroll, viewW, colW, totalCols, leftPad, rightPad)
  const cols = Math.max(1, totalCols)
  const extra = prev.to - prev.from - (need.to - need.from)
  if (
    prev.from >= 1
    && prev.to <= cols
    && prev.from <= need.from
    && prev.to >= need.to
    && extra <= keepExtra
  ) {
    return prev
  }
  const padTo = Math.max(slack, keepExtra - slack)
  let from = Math.max(1, Math.min(prev.from, need.from))
  if (from < need.from - keepExtra) from = Math.max(1, need.from - keepExtra)
  const to = Math.min(
    cols,
    prev.to >= need.to
      ? Math.min(prev.to, need.to + padTo)
      : need.to + padTo,
  )
  return { from, to }
}

/** 首屏和停稳窗口可以比视口多留的列。手势里不按这个拆左边。 */
export const RAIL_MOUNT_PAN_EXTRA = 12

/** 预显还剩这么多列就先扩窗，挂卡不压在右缘。 */
export const RAIL_MOUNT_RESERVE = 2

/** 预显已在 eager 里；实卡只再多留一列给提交延迟。 */
export const RAIL_MOUNT_LIVE_PAD = RAIL_MOUNT_RESERVE + 1

/** 扩窗一次多挂几列，避免每过一列都拆卡。 */
export const RAIL_MOUNT_GROW_AHEAD = 8

/** 实卡只铺到预显后一点，扩窗多出来的列先空着。 */
export function railLiveTo(
  eagerTo: number,
  mountTo: number,
  prevLive = 1,
): number {
  const mount = Math.max(1, mountTo)
  return Math.min(
    mount,
    Math.max(prevLive, Math.max(1, eagerTo) + RAIL_MOUNT_LIVE_PAD),
  )
}

/** 下手时一次补到这么远，滑起来少挂新卡。 */
export const RAIL_MOUNT_GRAB_AHEAD = 24

/** 首屏多挂一段，刚开滑少拆卡。 */
export const RAIL_MOUNT_BOOT_TO = RAIL_MOUNT_PAN_EXTRA + RAIL_MOUNT_GROW_AHEAD

/** 手势窗口已经盖住视口和右侧预显，且没有大到要挪走。 */
export function railMountColumnsCovered(
  prev: { from: number; to: number },
  need: { from: number; to: number },
  totalCols: number,
  keepExtra: number,
  reserve = 0,
): boolean {
  const cols = Math.max(1, totalCols)
  const extra = prev.to - prev.from - (need.to - need.from)
  return (
    prev.from >= 1
    && prev.to <= cols
    && prev.from <= need.from
    && prev.to >= need.to + reserve
    && extra <= keepExtra
  )
}

/** 手势里只扩到预显，不拆左边；停稳再收。 */
export function railMountColumnsPan(
  prev: { from: number; to: number },
  scroll: number,
  viewW: number,
  colW: number,
  totalCols: number,
  leftPad = 1,
  rightPad = 3,
  slack = 2,
): { from: number; to: number } {
  return railMountColumnsGrow(
    prev,
    scroll,
    viewW,
    colW,
    totalCols,
    leftPad,
    rightPad,
    slack,
  )
}

/** 手势里只扩不缩，停稳再用 sticky 收回。 */
export function railMountColumnsGrow(
  prev: { from: number; to: number },
  scroll: number,
  viewW: number,
  colW: number,
  totalCols: number,
  leftPad = 1,
  rightPad = 3,
  slack = 2,
): { from: number; to: number } {
  const next = railMountColumnsSticky(
    prev,
    scroll,
    viewW,
    colW,
    totalCols,
    leftPad,
    rightPad,
    slack,
  )
  const cols = Math.max(1, totalCols)
  const from = Math.max(1, Math.min(prev.from, next.from))
  let to = Math.min(cols, Math.max(prev.to, next.to))
  if (to > prev.to) to = Math.min(cols, to + RAIL_MOUNT_GROW_AHEAD)
  if (from === prev.from && to === prev.to) return prev
  return { from, to }
}

export function railColumnSlots(totalCols: number, colW: number): number[] {
  const cols = Math.max(1, totalCols)
  const width = Math.max(1, colW)
  const slots = Array.from({ length: cols }, (_, index) => index * width)
  return slots
}

/** 文章轨列间距，跟 cards.css 里 items-track 的 column-gap 对齐。 */
export const STORY_RAIL_COL_GAP = '0.75rem'

/** 整轨宽度按全列算，不靠 200 列栅。 */
export function storyRailTrackSize(
  total: number,
  gap = STORY_RAIL_COL_GAP,
): string {
  const cols = Math.max(1, total)
  if (cols <= 1) return 'var(--phantasi-story-w)'
  return `calc(${cols} * (var(--phantasi-story-w) + ${gap}) - ${gap})`
}

/** 实装窗夹在全列里，不算栅格模板。 */
export function storyMountWindow(
  from: number,
  to: number,
  total: number,
): { from: number; to: number } {
  const cols = Math.max(1, total)
  const start = Math.max(1, Math.min(from, cols))
  return { from: start, to: Math.max(start, Math.min(to, cols)) }
}

export function railLeadIndex(
  cards: ReadonlyArray<{ left: number; width: number }>,
  current: number,
  viewW: number,
  fade = CONVERSATION_FADE_PX,
): number {
  let fallback = -1
  let best = 0
  for (let i = 0; i < cards.length; i++) {
    const left = cards[i].left - current
    const visible =
      Math.min(left + cards[i].width, viewW) - Math.max(left, 0)
    if (visible <= 0.5) continue
    if (visible >= Math.min(fade, cards[i].width) - 0.5) return i
    if (visible > best) {
      best = visible
      fallback = i
    }
  }
  return fallback
}

export function railSeatScroll(
  cards: ReadonlyArray<{ left: number }>,
  focusIndex: number,
  overflowLeft = 0,
): number {
  if (cards.length === 0 || focusIndex < 0) return 0
  const origin = cards[0]?.left ?? 0
  const raw = Math.max(0, (cards[focusIndex]?.left ?? origin) - origin)
  if (focusIndex <= 0 || overflowLeft <= 0) return raw
  return Math.max(0, raw - overflowLeft)
}

/** 跟手用实卡位移，不走槽位座位。 */
export function railCardOffset(
  cards: ReadonlyArray<{ left: number }>,
  index: number,
): number {
  if (cards.length === 0 || index < 0) return 0
  const origin = cards[0]?.left ?? 0
  return Math.max(0, (cards[index]?.left ?? origin) - origin)
}

export function railMaxScroll(slots: readonly number[], overflowLeft = 0): number {
  if (slots.length <= 1) return 0
  return Math.max(0, (slots.at(-1) ?? 0) - Math.max(0, overflowLeft))
}

export function railSlotOffsets(
  cards: ReadonlyArray<{ left: number }>,
): number[] {
  if (cards.length === 0) return [0]
  const origin = cards[0].left
  const slots: number[] = []
  for (const card of cards) {
    const left = card.left - origin
    if (!slots.length || Math.abs(slots.at(-1)! - left) > 0.5) {
      slots.push(left)
    }
  }
  return slots
}

export function railSettleTau(
  distance: number,
  seating: boolean,
  cruise = false,
): number {
  if (!seating) return RAIL_FOLLOW_TAU
  const abs = Math.abs(distance)
  if (cruise) {
    if (abs > 160) return RAIL_CRUISE_SNAP_FAR_TAU
    if (abs > 56) return RAIL_CRUISE_SNAP_MID_TAU
    return RAIL_CRUISE_SNAP_NEAR_TAU
  }
  if (abs > 160) return RAIL_SNAP_FAR_TAU
  if (abs > 56) return RAIL_SNAP_MID_TAU
  return RAIL_SNAP_NEAR_TAU
}

/** 惯性直接积分，不再套一层跟手插值。出界就停。 */
export function railCoastStep(
  scroll: number,
  velocity: number,
  dt: number,
  max: number,
): { scroll: number; velocity: number } {
  if (dt <= 0 || velocity === 0) {
    return { scroll, velocity: 0 }
  }
  const next = scroll + velocity * dt
  if (next <= 0 || next >= max) {
    return {
      scroll: clampConversationScroll(next, max),
      velocity: 0,
    }
  }
  return {
    scroll: next,
    velocity: decayVelocity(velocity, dt, RAIL_FLING_TAU, RAIL_FLING_MIN_PX_S),
  }
}

export function isDiscreteWheel(event: WheelEvent): boolean {
  return event.deltaMode !== 0
}

function followRailAt(
  i: number,
  driveScroll: number,
  driveStops: readonly number[],
  followStops: readonly number[],
): number {
  const from = driveStops[i] ?? 0
  const to = driveStops[i + 1] ?? from
  if (to - from <= 0.5) return followStops[i] ?? 0
  const t = Math.min(1, Math.max(0, (driveScroll - from) / (to - from)))
  return (followStops[i] ?? 0) + t * ((followStops[i + 1] ?? 0) - (followStops[i] ?? 0))
}

/** 主动轨滚过各分组起点时，从动轨按实卡位移跟着走，不吸入槽位。 */
export function followRailScroll(
  driveScroll: number,
  driveStops: readonly number[],
  followStops: readonly number[],
  hint?: { i: number },
): number {
  const n = Math.min(driveStops.length, followStops.length)
  if (n === 0) return 0
  if (n === 1) return followStops[0] ?? 0
  const first = driveStops[0] ?? 0
  if (driveScroll <= first) {
    if (hint) hint.i = 0
    return followStops[0] ?? 0
  }
  const last = n - 2
  let i = last
  if (hint && hint.i >= 0 && hint.i <= last) {
    i = hint.i
    while (i < last && driveScroll > (driveStops[i + 1] ?? 0)) i += 1
    while (i > 0 && driveScroll < (driveStops[i] ?? 0)) i -= 1
  } else {
    let lo = 0
    let hi = last
    while (lo <= hi) {
      const mid = (lo + hi) >> 1
      const to = driveStops[mid + 1] ?? driveStops[mid] ?? 0
      if (driveScroll > to && mid !== last) {
        lo = mid + 1
      } else {
        i = mid
        hi = mid - 1
      }
    }
  }
  if (hint) hint.i = i
  return followRailAt(i, driveScroll, driveStops, followStops)
}

export function sourceAtScroll(
  scroll: number,
  stops: ReadonlyArray<{ id: number; start: number }>,
): number | null {
  if (stops.length === 0) return null
  let lo = 0
  let hi = stops.length - 1
  let id = stops[0]?.id ?? null
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    const start = stops[mid]?.start ?? 0
    if (scroll >= start - 0.5) {
      id = stops[mid]?.id ?? id
      lo = mid + 1
    } else {
      hi = mid - 1
    }
  }
  return id
}

/** 收回全屏时接着座定，不要从 0 再吸一次。 */
export function scrollFromTrackTransform(transform: string): number {
  const match = /translate3d\(\s*(-?[\d.]+)px/i.exec(transform)
  if (!match) return 0
  const x = Number(match[1])
  if (!Number.isFinite(x)) return 0
  return Math.max(0, -x)
}
