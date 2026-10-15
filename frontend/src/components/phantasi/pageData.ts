import type { PhantasiNoteDoc } from '../../types/phantasi'
import * as phantasiApi from '../../services/phantasiApi'
import { phantasiCacheKeys } from '../../services/phantasiCache'
import { requestCache } from '../../utils/requestCache'

export const NOTE_DOCS_CACHE_KEY = phantasiCacheKeys.noteDocs
const NOTE_DOCS_TTL = 60_000

export async function loadNoteDocs(
  signal?: AbortSignal,
): Promise<PhantasiNoteDoc[]> {
  const docs = await requestCache.fetch(
    NOTE_DOCS_CACHE_KEY,
    (transportSignal) => phantasiApi.listNoteDocs(transportSignal),
    NOTE_DOCS_TTL,
    false,
    signal,
  )
  signal?.throwIfAborted()
  return docs
}
