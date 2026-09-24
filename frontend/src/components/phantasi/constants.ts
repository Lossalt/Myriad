import { displayMediaUrl } from '../../utils/displayMediaUrl'

/** Canonical stored values. Display copy lives in locale JSON. */
const PHANTASI_FRIEND_LINK_CATEGORY = '友情链接'
export const PHANTASI_MINE_CATEGORY = '我'

const FRIEND_LINK_ALIASES = new Set([
  PHANTASI_FRIEND_LINK_CATEGORY,
  '友情連結',
  'friend_links',
  'friend-links',
  'friend links',
])

const MINE_ALIASES = new Set([
  PHANTASI_MINE_CATEGORY,
  'mine',
  'own',
  'me',
  'my',
])

function normalizeCategoryToken(value: string): string {
  return value.trim().toLowerCase()
}

export function isFriendLinkCategory(value: string | null | undefined): boolean {
  const raw = value?.trim() ?? ''
  if (!raw) return false
  return (
    FRIEND_LINK_ALIASES.has(raw) ||
    FRIEND_LINK_ALIASES.has(normalizeCategoryToken(raw))
  )
}

export function isMineCategory(value: string | null | undefined): boolean {
  const raw = value?.trim() ?? ''
  if (!raw) return false
  return MINE_ALIASES.has(raw) || MINE_ALIASES.has(normalizeCategoryToken(raw))
}

export function phantasiCategoryParts(
  category: string | null | undefined,
): string[] {
  if (!category) return []
  return category
    .split(',')
    .map((p) => p.trim())
    .filter(Boolean)
}

/** 本地笔记或 category 含「我」的公开源才算自有，可做文章级 SEO。 */
export function isOwnPhantasiSource(source: {
  source_type?: string | null
  category?: string | null
  admin_only?: boolean
} | null | undefined): boolean {
  if (!source) return false
  // admin_only 源不公开收录
  if (source.admin_only) return false
  if (source.source_type === 'note') return true
  return phantasiCategoryParts(source.category).some(isMineCategory)
}

/** 忽略预置分类后的第一个真实分类；排序与分类页标题共用。 */
export function phantasiMainCategory(
  category: string | null | undefined,
  fallback: string,
): string {
  const parts = phantasiCategoryParts(category)
  const main = parts.find(
    (c) => !isFriendLinkCategory(c) && !isMineCategory(c),
  )
  return main || fallback
}

export const DEFAULT_THEME_COLOR = '#6b7280'

const phantasiImageUrls = new Map<string, string | null>()
const PHANTASI_IMAGE_URL_CAP = 400

function resolvePhantasiImageUrl(imageUrl: string | null): string | null {
  if (!imageUrl) return null
  if (phantasiImageUrls.has(imageUrl)) return phantasiImageUrls.get(imageUrl) ?? null
  const next: string | null = displayMediaUrl(imageUrl)
  if (phantasiImageUrls.size >= PHANTASI_IMAGE_URL_CAP) {
    const first = phantasiImageUrls.keys().next().value
    if (first != null) phantasiImageUrls.delete(first)
  }
  phantasiImageUrls.set(imageUrl, next)
  return next
}

/** 仅 must-proxy 走 `/api/proxy/image`。 */
export function getIconUrl(iconUrl: string | null): string | null {
  return resolvePhantasiImageUrl(iconUrl)
}

/** 规则同 getIconUrl。 */
export function getImageUrl(imageUrl: string | null): string | null {
  return resolvePhantasiImageUrl(imageUrl)
}

const plainCache = new Map<string, string>()
const PLAIN_CACHE_CAP = 400

export function getPlainText(html: string | null, max = 200): string {
  if (!html) return ''
  const key = max === 200 ? html : `${max}\0${html}`
  const hit = plainCache.get(key)
  if (hit != null) return hit
  const text = html.replaceAll(/<[^>]*>/g, '').slice(0, max)
  if (plainCache.size >= PLAIN_CACHE_CAP) {
    const first = plainCache.keys().next().value
    if (first != null) plainCache.delete(first)
  }
  plainCache.set(key, text)
  return text
}

/** 只产出 #rrggbb，`${color}30` 才是合法 CSS。 */
export function normalizeThemeColor(
  color: string | null | undefined,
  fallback = DEFAULT_THEME_COLOR,
): string {
  if (!color || typeof color !== 'string') return fallback
  const t = color.trim()
  if (/^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(t)) {
    if (t.length === 4) {
      // #rgb → #rrggbb
      return `#${t[1]}${t[1]}${t[2]}${t[2]}${t[3]}${t[3]}`.toLowerCase()
    }
    return t.slice(0, 7).toLowerCase() // drop #rrggbbaa alpha
  }
  return fallback
}
