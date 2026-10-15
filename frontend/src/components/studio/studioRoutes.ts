/** 创作台地址。订阅类面板不在这里。 */

export const STUDIO_ROOT = '/studio'

export type StudioSection =
  | 'home'
  | 'notes'
  | 'comments'
  | 'media'
  | 'reviews'
  | 'categories'
  | 'import'
  | 'blog'

export type StudioLocation =
  | { kind: 'section'; section: StudioSection }
  | { kind: 'editor'; docId: number | 'new' }

const SECTION_PATHS: Record<Exclude<StudioSection, 'home'>, string> = {
  notes: '/studio/notes',
  comments: '/studio/comments',
  media: '/studio/media',
  reviews: '/studio/reviews',
  categories: '/studio/categories',
  import: '/studio/import',
  blog: '/studio/blog',
}

export function studioSectionPath(section: StudioSection): string {
  if (section === 'home') return STUDIO_ROOT
  return SECTION_PATHS[section]
}

export function studioNotePath(docId: number | 'new'): string {
  return `/studio/notes/${docId}`
}

function trimPath(pathname: string): string {
  if (pathname.length > 1 && pathname.endsWith('/')) {
    return pathname.replace(/\/+$/, '') || '/'
  }
  return pathname
}

export function parseStudioPath(pathname: string): StudioLocation | null {
  const path = trimPath(pathname)
  if (path === STUDIO_ROOT) return { kind: 'section', section: 'home' }
  if (path === '/studio/notes') return { kind: 'section', section: 'notes' }
  if (path === '/studio/notes/new') return { kind: 'editor', docId: 'new' }
  if (path.startsWith('/studio/notes/')) {
    const raw = path.slice('/studio/notes/'.length)
    if (!raw || raw.includes('/')) return null
    if (!/^[1-9]\d*$/.test(raw)) return null
    const id = Number(raw)
    if (!Number.isSafeInteger(id)) return null
    return { kind: 'editor', docId: id }
  }
  for (const [section, target] of Object.entries(SECTION_PATHS) as Array<[StudioSection, string]>) {
    if (path === target) return { kind: 'section', section }
  }
  return null
}

export function isStudioPath(pathname: string): boolean {
  return pathname === STUDIO_ROOT || pathname.startsWith(`${STUDIO_ROOT}/`)
}

export type StudioAccess = 'pending' | 'login' | 'home' | 'allow'

/** 与 App 里 RequireAuth requiresAdmin 同一口径：未登录去登录，非管理员回首页。 */
export function studioAccess(input: {
  hasChecked: boolean
  isAuthenticated: boolean
  isAdmin: boolean
}): StudioAccess {
  if (!input.hasChecked) return 'pending'
  if (!input.isAuthenticated) return 'login'
  if (!input.isAdmin) return 'home'
  return 'allow'
}
