import { requestCache } from '../utils/requestCache'

export const phantasiCacheKeys = {
  sources: 'phantasi:sources',
  sourceCatalog: 'phantasi:sources:catalog',
  sourceList: (view?: 'catalog', category?: string, board?: string) => {
    const base =
      view === 'catalog'
        ? phantasiCacheKeys.sourceCatalog
        : phantasiCacheKeys.sources
    const cat = category?.trim()
    const slice = board?.trim()
    const withCat = cat ? `${base}:cat:${cat}` : base
    return slice ? `${withCat}:board:${slice}` : withCat
  },
  stats: 'phantasi:stats',
  categories: 'phantasi:categories',
  noteDocs: 'phantasi:note-docs',
  item: (id: number) => `phantasi:item:${id}`,
} as const

export function invalidatePhantasiStatsCache(): void {
  requestCache.delete(phantasiCacheKeys.stats)
}

export function invalidatePhantasiSourcesCache(): void {
  requestCache.deleteByPrefix('phantasi:sources')
  invalidatePhantasiStatsCache()
}

export function invalidatePhantasiNoteDocsCache(): void {
  requestCache.delete(phantasiCacheKeys.noteDocs)
}
