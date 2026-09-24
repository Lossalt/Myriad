import { API_URL } from '../config'
import { proxyImageUrl } from './proxyImageUrl'
import { siteMediaUrl } from './siteMediaUrl'

/**
 * The one display rule for an image or media URL: platform media (also when
 * stored under an older site origin) is read from the current API origin;
 * another site's image goes through the image proxy only when it blocks
 * hotlinking. Stored values are never rewritten.
 */
export function displayMediaUrl(src: string, apiUrl: string = API_URL): string {
  const raw = src.trim()
  if (!raw || raw.startsWith('data:') || raw.startsWith('blob:')) return raw
  const local = siteMediaUrl(raw, apiUrl)
  if (local !== raw || raw.startsWith('/')) return local
  return proxyImageUrl(raw) ?? raw
}
