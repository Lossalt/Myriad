import { currentCopy } from '../../i18n/localeCopy'
import { apiService } from '../../services/api'
import { MeropeApiError, meropeError, PREFIX, RIG_MUTATION_TIMEOUT_MS } from './api'

/** A redraw takes as long as a portrait drawn from scratch. */
const UPRIGHT_TIMEOUT_MS = 15 * 60 * 1000

/** What a redraw square to the camera would put right in an uploaded picture. */
export type UprightReason = 'turned' | 'backdrop' | 'held' | 'arms'

const UPRIGHT_REASONS: readonly UprightReason[] = ['turned', 'backdrop', 'held', 'arms']

/** What a redraw kept of the picture it was redrawn from. */
export interface UprightCheck {
  sameCharacter: boolean
  sameStyle: boolean
  upright: boolean
  differences: string[]
}

/**
 * How a set's picture is framed: what of it the rig cannot take. No outfit
 * is the worn bust (the master portrait).
 */
export async function readPortraitFraming(outfitId?: string): Promise<UprightReason[]> {
  try {
    const data = await apiService.post<{ reasons?: unknown }>(
      `${PREFIX}/portrait/framing`,
      { outfitId },
      { timeout: RIG_MUTATION_TIMEOUT_MS },
    )
    return Array.isArray(data.reasons)
      ? data.reasons.filter((reason): reason is UprightReason =>
          UPRIGHT_REASONS.includes(reason as UprightReason),
        )
      : []
  } catch (reason) {
    throw meropeError(reason, currentCopy().merope.uprightReadFailed)
  }
}

/**
 * The set's picture redrawn square to the camera in its own style, not yet
 * stored: the page uploads it to the set when the owner takes it.
 */
export async function drawUprightPortrait(
  outfitId: string | undefined,
  language: string,
): Promise<{ image: Blob; check: UprightCheck | null }> {
  try {
    const data = await apiService.post<{
      image?: unknown
      mediaType?: unknown
      check?: unknown
    }>(
      `${PREFIX}/portrait/upright`,
      { outfitId, language },
      { timeout: UPRIGHT_TIMEOUT_MS },
    )
    if (typeof data.image !== 'string' || !data.image) {
      throw new MeropeApiError(currentCopy().merope.uprightFailed, 502)
    }
    const binary = atob(data.image)
    const bytes = new Uint8Array(binary.length)
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
    const type = typeof data.mediaType === 'string' ? data.mediaType : 'image/png'
    return { image: new Blob([bytes], { type }), check: readUprightCheck(data.check) }
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    throw meropeError(reason, currentCopy().merope.uprightFailed)
  }
}

function readUprightCheck(value: unknown): UprightCheck | null {
  if (!value || typeof value !== 'object') return null
  const check = value as Record<string, unknown>
  if (
    typeof check.sameCharacter !== 'boolean' ||
    typeof check.sameStyle !== 'boolean' ||
    typeof check.upright !== 'boolean'
  ) {
    return null
  }
  return {
    sameCharacter: check.sameCharacter,
    sameStyle: check.sameStyle,
    upright: check.upright,
    differences: Array.isArray(check.differences)
      ? check.differences.filter((note): note is string => typeof note === 'string')
      : [],
  }
}
