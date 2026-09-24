/** skin 不进口。 */
import type {
  AddSourceInput,
  PhantasiSource,
  UpdateSourceRequest,
} from '../../types/phantasi'
import { useCallback, useEffect, useRef, useState } from 'react'

import { ApiError } from '../../services/api'
import * as phantasiApi from '../../services/phantasiApi'
import { phantasiItemState } from '../../utils/phantasiItemState'
import { RequestTurn } from './logic/requestTurn'
import { connectSourceUpdates } from './logic/sourceConnection'
import { reportPhantasiError } from './phantasiNotice'

/** The server already has a fetch of this source in flight. */
export function isRefreshInProgress(error: unknown): boolean {
  return error instanceof ApiError && error.code === 'source_refresh_in_progress'
}

function applyReadMutation(
  source: PhantasiSource,
  itemId: number,
  isRead: boolean,
  sourceId?: number,
): PhantasiSource {
  if (itemId === 0) {
    if (
      source.unread_count === 0 &&
      !source.recent_items?.some((item) => !item.is_read)
    ) {
      return source
    }
    return {
      ...source,
      unread_count: 0,
      recent_items: source.recent_items?.map((item) =>
        item.is_read ? item : { ...item, is_read: true },
      ),
    }
  }
  const belongs =
    sourceId === source.id ||
    Boolean(source.recent_items?.some((item) => item.id === itemId))
  if (!belongs) return source
  const hit = source.recent_items?.find((item) => item.id === itemId)
  if (hit?.is_read === isRead) return source
  return {
    ...source,
    unread_count: Math.max(0, source.unread_count + (isRead ? -1 : 1)),
    recent_items: source.recent_items?.map((item) =>
      item.id === itemId ? { ...item, is_read: isRead } : item,
    ),
  }
}

export function usePhantasiSources(
  isAuthenticated: boolean,
  labels: { loadFailed: string; refreshFailed: string },
  setError: (message: string) => void,
  scope: 'feeds' | 'notes' | 'sites' | 'all' | 'catalog' | 'none' = 'all',
) {
  const [sources, setSources] = useState<PhantasiSource[]>([])
  const [sourcesLoaded, setSourcesLoaded] = useState(false)
  const [booting, setBooting] = useState(true)

  const sourceRequest = useRef(0)
  const sourceTurns = useRef(new RequestTurn())

  const loadSources = useCallback(async (forceRefresh = false) => {
    if (scope === 'none') {
      setSources([])
      setSourcesLoaded(false)
      return
    }
    const request = ++sourceRequest.current
    const signal = sourceTurns.current.begin()
    try {
      const data = await phantasiApi.getSources(undefined, {
        signal,
        forceRefresh,
        view: scope === 'catalog' ? 'catalog' : undefined,
        board:
          scope === 'feeds' || scope === 'notes' || scope === 'sites'
            ? scope
            : undefined,
      })
      if (signal.aborted || request !== sourceRequest.current) return
      setSourcesLoaded(true)
      setSources(data)
    } catch (err) {
      if (signal.aborted || request !== sourceRequest.current) return
      reportPhantasiError(err, labels.loadFailed, setError)
    }
  }, [labels.loadFailed, scope, setError])

  useEffect(() => {
    const unsubscribe = phantasiItemState.subscribeMutations((id, patch, sourceId) => {
      if (typeof patch.is_read === 'boolean') {
        setSources((prev) =>
          prev.map((source) =>
            applyReadMutation(source, id, patch.is_read!, sourceId),
          ),
        )
      }
    })
    return unsubscribe
  }, [])

  useEffect(() => {
    if (scope === 'none') {
      sourceTurns.current.cancel()
      sourceRequest.current++
      setSources([])
      setSourcesLoaded(false)
      setBooting(false)
      return () => {
        sourceTurns.current.cancel()
        sourceRequest.current++
      }
    }
    let cancelled = false
    setBooting(true)
    void loadSources().finally(() => {
      if (!cancelled) setBooting(false)
    })
    return () => {
      cancelled = true
      sourceTurns.current.cancel()
      sourceRequest.current++
    }
  }, [loadSources, scope])

  const refreshRef = useRef(() => {})
  refreshRef.current = () => {
    void loadSources(true)
  }

  useEffect(() => {
    if (!isAuthenticated || scope === 'none') return
    return connectSourceUpdates(
      phantasiApi.createPhantasiWebSocket,
      () => refreshRef.current(),
    )
  }, [isAuthenticated, scope])

  const reloadBoard = useCallback(() => {
    void loadSources(true)
  }, [loadSources])

  const updateSource = useCallback(
    async (id: number, data: UpdateSourceRequest) => {
      const updated = await phantasiApi.updateSource(id, data)
      setSources((prev) =>
        prev.map((source) => (source.id === id ? updated : source)),
      )
    },
    [],
  )

  const importOpml = useCallback(
    async (content: string, signal?: AbortSignal) => {
      const result = await phantasiApi.importOpml(
        content,
        undefined,
        signal ? { signal } : undefined,
      )
      if (!signal?.aborted) reloadBoard()
      return {
        imported: result.imported || 0,
        skipped: result.skipped || 0,
      }
    },
    [reloadBoard],
  )

  const removeSources = useCallback(
    async (ids: number[]) => {
      if (ids.length === 0) return
      const named = new Map(sources.map((source) => [source.id, source.name]))
      const results = await Promise.allSettled(
        ids.map((id) => phantasiApi.deleteSource(id)),
      )
      const dropped = ids.filter((_, index) => results[index]?.status === 'fulfilled')
      if (dropped.length > 0) {
        const gone = new Set(dropped)
        setSources((prev) =>
          Iterator.from(prev)
            .filter((source) => !gone.has(source.id))
            .toArray(),
        )
      }
      const failed = ids.filter((_, index) => results[index]?.status === 'rejected')
      if (failed.length > 0) {
        const names = failed
          .map((id) => named.get(id)?.trim() || `#${id}`)
          .join('、')
        throw new Error(names)
      }
    },
    [sources],
  )

  const addSource = useCallback(
    async ({
      url,
      name,
      category,
      icon,
      sourceType,
      feedType,
      notionToken,
    }: AddSourceInput) => {
      const source = await phantasiApi.addSource({
        url,
        name,
        category,
        source_type: sourceType,
        feed_type: feedType,
        extra_config: notionToken ? { token: notionToken } : undefined,
      })
      if (icon && source.id) {
        const updated = await phantasiApi.updateSource(source.id, { icon })
        setSources((prev) => [...prev, updated])
      } else {
        setSources((prev) => [...prev, source])
      }
    },
    [],
  )

  const discoverSource = useCallback(
    async (url: string, signal?: AbortSignal) => {
      return phantasiApi.discoverSource(url.trim(), undefined, { signal })
    },
    [],
  )

  const generateStyleTags = useCallback(
    async (sourceId: number, signal?: AbortSignal) => {
      return phantasiApi.generateStyleTags(sourceId, signal)
    },
    [],
  )

  const refreshSource = useCallback(
    async (sourceId: number) => {
      try {
        await phantasiApi.refreshSource(sourceId)
      } catch (err) {
        // Another fetch of this source is in flight; its result is what this
        // refresh wanted, and the reload below picks it up.
        if (!isRefreshInProgress(err))
          reportPhantasiError(err, labels.refreshFailed, setError)
      } finally {
        reloadBoard()
      }
    },
    [reloadBoard, labels.refreshFailed, setError],
  )

  const refreshSources = useCallback(
    async (ids: number[]) => {
      if (ids.length === 0) return
      try {
        await phantasiApi.refreshSources(ids)
      } catch (err) {
        reportPhantasiError(err, labels.refreshFailed, setError)
      } finally {
        reloadBoard()
      }
    },
    [reloadBoard, labels.refreshFailed, setError],
  )

  return {
    sources,
    setSources,
    sourcesLoaded,
    booting,
    loadSources,
    reloadBoard,
    addSource,
    updateSource,
    importOpml,
    removeSources,
    refreshSource,
    refreshSources,
    discoverSource,
    generateStyleTags,
  }
}
