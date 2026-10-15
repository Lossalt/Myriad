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
