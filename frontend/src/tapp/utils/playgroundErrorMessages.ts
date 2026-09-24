import { currentCopy } from '../../i18n/localeCopy'
import { resolveErrorCode } from '../../utils/errorCodes'

/**
 * Copy for failures the backend never labels (the request never reached the
 * Playground handler, or the browser gave up). Everything the backend reports
 * arrives with a `playground_*` code and reads from the shared `errors.byCode`.
 */
export interface PlaygroundErrorCopy {
  playgroundTimeoutHint: string
  playgroundServerErrorHint: string
  playgroundGenerateFailed: string
  playgroundNetworkHint: string
  playgroundErrorDetail: string
  playgroundRuntimeError: string
  playgroundUnknownError?: string
}

export interface MapPlaygroundErrorOpts {
  userCancelled?: boolean
  /** API `code` (`playground_*` from the Playground handler, or a generic one). */
  code?: string
  /** HTTP status when the failure came from an HTTP response. */
  status?: number
  format?: (template: string, params: Record<string, string | number>) => string
}

type Format = (template: string, params: Record<string, string | number>) => string

const DETAIL_MAX = 720

function defaultFormat(
  template: string,
  params: Record<string, string | number>,
): string {
  return Object.entries(params).reduce(
    (s, [k, v]) => s.replaceAll(`{${k}}`, String(v)),
    template,
  )
}

function truncateDetail(text: string, max = DETAIL_MAX): string {
  const t = text.replaceAll(/\s+/g, ' ').trim()
  if (t.length <= max) return t
  return `${t.slice(0, max - 1)}…`
}

function compose(
  primary: string,
  detail: string | undefined,
  detailTemplate: string,
  format: Format,
): string {
  const d = detail ? truncateDetail(detail) : ''
  if (!d || d === primary || primary.includes(d)) return primary
  if (detailTemplate.includes('{detail}')) {
    return `${primary}\n${format(detailTemplate, { detail: d })}`
  }
  return `${primary}\n${d}`
}

function extractValidationDetail(raw: string): string {
  const m = raw.match(/did not pass validation after \d+ attempts?: (.+)$/i)
  if (m?.[1]) return m[1].trim()
  const colon = raw.indexOf(': ')
  if (colon > 0 && /validation/i.test(raw.slice(0, colon))) {
    return raw.slice(colon + 2).trim()
  }
  return raw
}

/** The backend detail worth showing under the code copy, per code. */
function codeDetail(code: string, raw: string): string | undefined {
  switch (code) {
    case 'playground_validation_failed':
      return extractValidationDetail(raw)
    case 'playground_payload_too_large':
    case 'playground_bad_request':
      return raw.replaceAll(/^HTTP\s*\d{3}\s*:?\s*/gi, '').trim() || undefined
    default:
      return undefined
  }
}

/**
 * Responses that never reached the Playground handler (auth middleware, CSRF,
 * rate limiter, body limit) carry only a status; read them as the Playground
 * code the handler itself would have used for that status.
 */
function playgroundCodeForStatus(status: number | undefined, raw: string): string | undefined {
  switch (status) {
    case 401:
      return 'playground_auth_required'
    case 403:
      // A CSRF rejection means the session needs refreshing, not admin rights.
      return /csrf/i.test(raw) ? 'playground_auth_required' : 'playground_admin_required'
    case 413:
      return 'playground_payload_too_large'
    case 422:
      return 'playground_validation_failed'
    case 429:
      return 'playground_rate_limited'
    default:
      return undefined
  }
}

function sharedCopy(code: string | undefined): string | undefined {
  if (!code) return undefined
  const table: Readonly<Record<string, string | undefined>> =
    currentCopy().errors.byCode
  return table[code]
}

function isTimeoutText(raw: string): boolean {
  return (
    /^timeouterror$/i.test(raw) ||
    /^aborterror$/i.test(raw) ||
    /timed?\s*out/i.test(raw) ||
    /\btimeout\b/i.test(raw) ||
    /the operation was aborted/i.test(raw)
  )
}

function isNetworkText(raw: string): boolean {
  return (
    /failed to fetch/i.test(raw) ||
    /networkerror/i.test(raw) ||
    /load failed/i.test(raw) ||
    /network request failed/i.test(raw) ||
    /net::err_/i.test(raw)
  )
}

export function mapPlaygroundGenerateError(
  message: string,
  copy: PlaygroundErrorCopy,
  opts?: MapPlaygroundErrorOpts,
): string {
  const format = opts?.format ?? defaultFormat
  const raw = (message || '').trim()

  if (opts?.userCancelled) {
    return sharedCopy('playground_cancelled') || copy.playgroundTimeoutHint
  }

  // 1. The Playground handler's own code (HTTP body or SSE error event).
  const explicit = opts?.code?.trim() || undefined
  const playgroundCode = explicit?.startsWith('playground_')
    ? explicit
    : playgroundCodeForStatus(opts?.status, raw)
  const playgroundCopy = sharedCopy(playgroundCode)
  if (playgroundCode && playgroundCopy) {
    return compose(
      playgroundCopy,
      codeDetail(playgroundCode, raw),
      copy.playgroundErrorDetail,
      format,
    )
  }

  if (!raw) return copy.playgroundGenerateFailed

  // 2. Browser-side failures: no response, so no code. The Playground wording
  //    ("your project and prompt were kept — Retry") is specific to this page.
  if (isTimeoutText(raw)) return copy.playgroundTimeoutHint
  if (isNetworkText(raw)) return copy.playgroundNetworkHint

  // 3. Any other coded failure: the shared table, same as everywhere else.
  const code = resolveErrorCode(explicit, raw)
  const otherCopy = sharedCopy(code)
  if (otherCopy) {
    return compose(otherCopy, raw, copy.playgroundErrorDetail, format)
  }

  const status = opts?.status ?? 0
  if (status >= 500 || /^HTTP\s*5\d\d\b/i.test(raw)) {
    return compose(
      copy.playgroundServerErrorHint,
      raw.startsWith('HTTP') ? raw : undefined,
      copy.playgroundErrorDetail,
      format,
    )
  }

  const looksLocalized = /[\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Han}]/u.test(raw) || raw.length > 40

  if (looksLocalized && !/^HTTP\s*\d+/i.test(raw) && !/^[a-z]{2,}Error$/i.test(raw)) {
    if (/^[\w .:/-]{1,48}$/.test(raw) && !/\s{2,}/.test(raw) && raw.split(' ').length <= 4) {
      return compose(copy.playgroundGenerateFailed, raw, copy.playgroundErrorDetail, format)
    }
    return raw
  }

  return compose(copy.playgroundGenerateFailed, raw, copy.playgroundErrorDetail, format)
}

export function mapPlaygroundRuntimeError(
  message: string,
  copy: Pick<
    PlaygroundErrorCopy,
    'playgroundRuntimeError' | 'playgroundUnknownError'
  >,
  format: Format = defaultFormat,
): string {
  const raw =
    (message || '').trim() || copy.playgroundUnknownError || ''
  if (!raw) {
    return copy.playgroundRuntimeError.includes('{message}')
      ? format(copy.playgroundRuntimeError, { message: '?' })
      : copy.playgroundRuntimeError
  }
  if (copy.playgroundRuntimeError.includes('{message}')) {
    return format(copy.playgroundRuntimeError, {
      message: truncateDetail(raw, 480),
    })
  }
  return raw
}
