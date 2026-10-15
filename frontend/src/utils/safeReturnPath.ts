/** 登录回跳只收本站路径。协议相对地址和站外 URL 丢掉。 */

const CONTROL = /[\u0000-\u001F\u007F\\]/

export function safeSameOriginPath(
  raw: string | null | undefined,
  origin: string,
): string | null {
  if (raw == null) return null
  const trimmed = raw.trim()
  if (!trimmed || CONTROL.test(trimmed)) return null
  let url: URL
  try {
    url = new URL(trimmed, origin)
  } catch {
    return null
  }
  if (url.origin !== origin) return null
  if (!url.pathname.startsWith('/') || url.pathname.startsWith('//')) return null
  if (url.pathname === '/login' || url.pathname.startsWith('/login/')) return null
  return `${url.pathname}${url.search}${url.hash}`
}

function isEmbedLocation(path: string): boolean {
  const pathname = path.split(/[?#]/, 1)[0] ?? path
  return pathname === '/embed' || pathname.startsWith('/embed/')
}

/**
 * 回跳顺序：`return` query、同源父页、同源 referrer。
 * 都没有时落到 `/blog/`。嵌入页自己的地址不能当文章回跳。
 */
export function articleReturnPath(input: {
  origin: string
  search: string
  referrer: string
  parentHref?: string | null
}): string {
  const query = input.search.startsWith('?') ? input.search.slice(1) : input.search
  const requested = safeSameOriginPath(new URLSearchParams(query).get('return'), input.origin)
  if (requested && !isEmbedLocation(requested)) return requested
  const parent = safeSameOriginPath(input.parentHref, input.origin)
  if (parent && !isEmbedLocation(parent)) return parent
  const referrer = safeSameOriginPath(input.referrer, input.origin)
  if (referrer && !isEmbedLocation(referrer)) return referrer
  return '/blog/'
}

const LOGIN_RETURN_KEY = 'login_return'

/** 密码登录读 query；OAuth 离开本页后只剩 sessionStorage。空值会清掉旧回跳。 */
export function rememberLoginReturn(path: string | null): void {
  if (typeof sessionStorage === 'undefined') return
  if (!path) {
    sessionStorage.removeItem(LOGIN_RETURN_KEY)
    return
  }
  sessionStorage.setItem(LOGIN_RETURN_KEY, path)
}

export function consumeLoginReturn(origin: string): string | null {
  if (typeof sessionStorage === 'undefined') return null
  const raw = sessionStorage.getItem(LOGIN_RETURN_KEY)
  sessionStorage.removeItem(LOGIN_RETURN_KEY)
  return safeSameOriginPath(raw, origin)
}

export function loginHref(returnPath: string | null): string {
  if (!returnPath) return '/login'
  return `/login?return=${encodeURIComponent(returnPath)}`
}
