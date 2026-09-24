const PROBE_ORIGIN = 'https://site.invalid'

/**
 * Whether a server-supplied navigate target stays inside this site.
 *
 * The backend already limits `router.navigate` to known route prefixes; this
 * is the browser's own check, so a protocol-relative (`//host`), backslash
 * (`/\host`) or absolute URL never reaches the router.
 */
export function isInSitePath(path: string): boolean {
  if (
    !path.startsWith('/') ||
    path.startsWith('//') ||
    path.startsWith('/\\')
  ) {
    return false
  }
  try {
    return new URL(path, PROBE_ORIGIN).origin === PROBE_ORIGIN
  } catch {
    return false
  }
}
