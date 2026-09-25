import { API_URL } from '../config'
import { currentCopy } from '../i18n/localeCopy'
import { requestPathname } from './aiRequestTimeout.mjs'

export const AI_CONFIGURATION_REQUIRED_EVENT = 'myriad:ai-configuration-required'
type Capability = 'standard' | 'chat' | 'pro' | 'persona' | 'personaName' | 'image'

/** Only generation operations: saved content and configuration remain accessible. */
function requiredCapability(url: string, init: RequestInit): Capability | undefined {
  if ((init.method ?? 'GET').toUpperCase() !== 'POST') return
  const path = requestPathname(url)
  if (/^\/api\/agent\/process(?:\/stream)?$/.test(path)) {
    try {
      if (typeof init.body === 'string' && JSON.parse(init.body).context?.mode === 'chat') return 'chat'
    } catch { /* The endpoint owns request validation. */ }
    return 'standard'
  }
  if (path === '/api/phantasiai/notes/edit') return 'standard'
  if (path === '/api/agent/persona/name') return 'personaName'
  if (path === '/api/agent/persona/visual-from-portrait') return 'pro'
  if (/^\/api\/agent\/persona\/(draft|import|visual-design)$/.test(path)) return 'persona'
  if (/^\/api\/tapp-playground\/generate(?:-stream)?$/.test(path)) return 'pro'
  if (/^\/api\/merope\/rig\/(portrait|avatar)$/.test(path)) return 'image'
  if (/^\/api\/agent\/(clarify|tasks\/[^/]+\/answer(?:\/stream)?)$/.test(path)) return 'standard'
  if (/^\/api\/phantasiai\/(items\/[^/]+\/(annotations|podcast)\/regenerate|sources\/[^/]+\/style-tags)$/.test(path)) return 'standard'
  if (/^\/api\/reports\/(platform|generate-all)$/.test(path)) return 'standard'
  return undefined
}

/** Recheck on each operation so saving/clearing configuration takes effect immediately. */
export async function checkAiConfiguration(
  url: string,
  init: RequestInit = {},
  fetcher: typeof fetch = fetch,
): Promise<Response | undefined> {
  init.signal?.throwIfAborted()
  const capability = requiredCapability(url, init)
  if (capability) {
    const response = await fetcher(`${API_URL}/api/config/public`, {
      credentials: 'include',
      signal: init.signal ?? AbortSignal.timeout(15000),
      cache: 'no-store',
    })
    init.signal?.throwIfAborted()
    if (!response.ok) return response
    const config = await response.json()
    init.signal?.throwIfAborted()
    // An older backend without capability metadata still owns validation.
    if (config.aiAvailability?.[capability] === false) {
      if (typeof window !== 'undefined') {
        window.dispatchEvent(new Event(AI_CONFIGURATION_REQUIRED_EVENT))
      }
      return Response.json({
        code: 'ai_not_configured',
        error: currentCopy().errors.aiNotConfigured,
      }, { status: 409 })
    }
  }
}

export async function fetchWithAiConfiguration(
  url: string,
  init: RequestInit = {},
  fetcher: typeof fetch = fetch,
): Promise<Response> {
  const blocked = await checkAiConfiguration(url, init, fetcher)
  if (blocked) return blocked
  init.signal?.throwIfAborted()
  const response = await fetcher(url, init)
  if (!response.ok && response.headers.get('content-type')?.includes('application/json')) {
    const body = await response.clone().json().catch(() => null)
    if (body?.code === 'ai_not_configured' && typeof window !== 'undefined') {
      window.dispatchEvent(new Event(AI_CONFIGURATION_REQUIRED_EVENT))
    }
  }
  return response
}
