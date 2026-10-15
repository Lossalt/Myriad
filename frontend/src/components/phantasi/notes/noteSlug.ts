import { ApiError } from '../../../services/api'

const SLUG_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/
export const MAX_NOTE_SLUG_CHARS = 80

/**
 * 空串交给后端生成。非法输入返回 undefined，调用方不要写进 PUT。
 * 合法结果是小写 slug，或 null（清空，发布时自动生成）。
 */
export function noteSlugWire(value: string | null | undefined): string | null | undefined {
  const raw = value?.trim().toLowerCase() ?? ''
  if (!raw) return null
  if ([...raw].length > MAX_NOTE_SLUG_CHARS || !SLUG_PATTERN.test(raw)) return undefined
  return raw
}

export function noteSlugInvalid(value: string | null | undefined): boolean {
  return noteSlugWire(value) === undefined
}

/** 优先用发布响应里的 link；没有时按 slug 拼博客地址。 */
export function publishedNoteHref(entry: {
  link?: string | null
  slug?: string | null
}): string | null {
  const link = entry.link?.trim()
  if (link) return link
  const slug = entry.slug?.trim()
  if (!slug) return null
  return `/blog/posts/${encodeURIComponent(slug)}/`
}

/** 409 里 revision 冲突的文案不含 slug。地址占用才带 slug。 */
export function isSlugTakenError(error: unknown): boolean {
  if (!(error instanceof ApiError) || error.status !== 409) return false
  const blob = `${error.code ?? ''} ${error.message}`.toLowerCase()
  return blob.includes('slug')
}
