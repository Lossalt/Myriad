/** skin 不进口。 */
import type { PhantasiItemPreview, PhantasiSource } from '../../types/phantasi'
import type { PhantasiBoard, SourceSortMode } from './logic/board'
import type { FeedStory, FeedStorySlot } from './logic/feedStories'
import type { HomeBoardNote } from './logic/homeBoard'
import type { PhantasiViewerRole } from './logic/score'
import { startTransition, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'

import { useI18n } from '../../contexts/I18nContext'

import {
  collectSourceCategories,
  filterSourcesByQuery,
  sortSourcesForBoard,
  sourcesForBoard,
} from './logic/board'
import {
  coverFeedIndices,
  FEEDS_ARTICLE_MAX,
  isAggregateFeedId,
  isTopicFeedId,
  latestFeedStories,
  reuseFeedStories,
  stitchStoriesBySources,
  topicFeedId,
  topicFeedStories,
} from './logic/feedStories'
import { noteSourceKey, noteSourceStamp } from './logic/homeBoard'
import {
  loadBoardNotes,
  loadFeedStories,
  loadTopicCatalog,
  loadTopicStories,
  peekFeedStories,
  peekFeedStoriesLoose,
} from './pageData'
import { useArticleFlags } from './useArticleFlags'

export function useBoardCatalog(
  sources: PhantasiSource[],
  board: PhantasiBoard,
  searchQuery: string,
  sortMode: SourceSortMode,
  role: PhantasiViewerRole,
  now: number,
) {
  const { locale } = useI18n()
  const categories = useMemo(() => collectSourceCategories(sources), [sources])
  const filtered = useMemo(
    () => filterSourcesByQuery(sourcesForBoard(sources, board), searchQuery),
    [sources, board, searchQuery],
  )
  const sorted = useMemo(
    () => sortSourcesForBoard(filtered, sortMode, role, now, locale),
    [filtered, sortMode, role, now, locale],
  )
  return { categories, filtered, sorted }
}

export function useBoardNotes(
  board: PhantasiBoard,
  sources: Array<{
    id: number
    source_type: string
    item_count?: number
    last_success_at?: number | null
  }>,
  epoch = 0,
) {
  const [loading, setLoading] = useState(board === 'notes')
  const [failed, setFailed] = useState(false)
  const [attempt, setAttempt] = useState(0)
  const [pageLimit, setPageLimit] = useState(1)
  const [hasMore, setHasMore] = useState(false)
  const loadMore = useCallback(() => setPageLimit(value => value + 1), [])
  const retry = useCallback(() => setAttempt(value => value + 1), [])
  const flags = useArticleFlags()
  const flagsRevision = flags.getSnapshot()
  const key = useMemo(() => noteSourceKey(sources), [sources])
  const stamp = useMemo(() => noteSourceStamp(sources), [sources])
  const sourcesRef = useRef(sources)
  sourcesRef.current = sources
  const [rawNotes, setNotes] = useState<HomeBoardNote[]>([])
  useEffect(() => { setPageLimit(1); setHasMore(false) }, [board, key, stamp, epoch])

  useEffect(() => {
    if (board !== 'notes') { setLoading(false); return }
    setFailed(false)
    if (!key) {
      setNotes([])
      setLoading(false)
      return
    }
    setLoading(true)
    const controller = new AbortController()
    const publish = (next: HomeBoardNote[]) => {
      if (!controller.signal.aborted) startTransition(() => setNotes(next))
    }
    void loadBoardNotes(sourcesRef.current, controller.signal, publish, pageLimit, (more) => {
      if (!controller.signal.aborted) setHasMore(more)
    })
      .then((next) => {
        if (!controller.signal.aborted) setNotes(next)
      })
      .catch(() => {
        if (!controller.signal.aborted) setFailed(true)
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false)
      })
    return () => {
      controller.abort()
    }
    // stamp：源集合或抓取结果变了才重拉。epoch：发布后条数可能不变，但缓存已失效。
  }, [board, key, stamp, epoch, attempt, pageLimit])

  const notes = useMemo(() => {
    if (board !== 'notes') return []
    return rawNotes.map((note) => flags.project(note))
  }, [board, flags, flagsRevision, rawNotes])
  return { notes, loading, failed, retry, hasMore, loadMore }
}

export function useFeedStories(
  board: PhantasiBoard,
  sources: readonly PhantasiSource[],
  onToggleStar?: (item: PhantasiItemPreview) => void | false | Promise<void | false>,
): {
  stories: FeedStory[]
  onStar: (item: PhantasiItemPreview) => void
  jump: (sourceId: number) => void
  holdStories: () => void
  releaseStories: () => void
  railEpoch: string
  topicCards: string[]
} {
  const flags = useArticleFlags()
  const flagsRevision = flags.getSnapshot()
  const [topicCards, setTopicCards] = useState<string[]>([])
  useEffect(() => {
    if (board !== 'feeds') {
      setTopicCards([])
      return
    }
    const controller = new AbortController()
    void loadTopicCatalog(controller.signal)
      .then((catalog) => {
        if (!controller.signal.aborted) setTopicCards(catalog.cards)
      })
      .catch(() => {
        if (!controller.signal.aborted) setTopicCards([])
      })
    return () => controller.abort()
  }, [board])
  const idKey = useMemo(
    () => sources.map((source) => source.id).join(','),
    [sources],
  )
  const stampKey = useMemo(
    () =>
      sources
        .map((source) => `${source.id}:${source.last_success_at ?? 0}`)
        .join(','),
    [sources],
  )
  const lastIndex = Math.max(0, sources.length - 1)
  const [cover, setCover] = useState<number[]>(() =>
    coverFeedIndices([], 0, lastIndex, 3),
  )
  const [tick, setTick] = useState(0)
  const slotsRef = useRef<Map<number, FeedStorySlot>>(new Map())
  const topicStoriesRef = useRef(new Map<string, FeedStory[]>())
  const topicInflightRef = useRef(new Set<string>())
  const topicGenerationRef = useRef(0)
  const topicCardsRef = useRef(topicCards)
  topicCardsRef.current = topicCards
  const sourcesRef = useRef(sources)
  sourcesRef.current = sources
  const inflightRef = useRef(new Set<string>())
  const fetchLiveRef = useRef(true)
  const readControllerRef = useRef(new AbortController())
  const quietRef = useRef(false)
  const pendingBumpRef = useRef(false)
  const queuedRef = useRef(false)

  const bump = useCallback(() => {
    if (!fetchLiveRef.current) return
    if (quietRef.current) {
      pendingBumpRef.current = true
      return
    }
    if (queuedRef.current) return
    queuedRef.current = true
    queueMicrotask(() => {
      queuedRef.current = false
      if (!fetchLiveRef.current) return
      if (quietRef.current) {
        pendingBumpRef.current = true
        return
      }
      setTick((value) => value + 1)
    })
  }, [])

  const holdStories = useCallback(() => {
    quietRef.current = true
  }, [])

  const releaseStories = useCallback(() => {
    if (!quietRef.current && !pendingBumpRef.current) return
    quietRef.current = false
    if (!pendingBumpRef.current) return
    pendingBumpRef.current = false
    bump()
  }, [bump])

  useEffect(() => {
    setCover(coverFeedIndices([], 0, lastIndex, 3))
  }, [board, lastIndex, idKey])

  const jump = useCallback((sourceId: number) => {
    const list = sourcesRef.current
    if (isAggregateFeedId(sourceId)) {
      setCover((current) => coverFeedIndices(current, 0, list.length - 1, 3))
      if (isTopicFeedId(sourceId)) {
        const topic = topicCardsRef.current[-2 - sourceId]
        if (
          topic
          && !topicStoriesRef.current.has(topic)
          && !topicInflightRef.current.has(topic)
        ) {
          const generation = topicGenerationRef.current
          const signal = readControllerRef.current.signal
          topicInflightRef.current.add(topic)
          void loadTopicStories(topic, signal)
            .then((items) => {
              if (
                fetchLiveRef.current
                && !signal.aborted
                && generation === topicGenerationRef.current
              ) {
                topicStoriesRef.current.set(topic, items)
                bump()
              }
            })
            .catch(() => {})
            .finally(() => {
              if (generation === topicGenerationRef.current) {
                topicInflightRef.current.delete(topic)
              }
            })
        }
      }
      return
    }
    const index = list.findIndex((source) => source.id === sourceId)
    if (index < 0) return
    setCover((current) =>
      coverFeedIndices(current, index, list.length - 1, 3),
    )
  }, [bump])

  useLayoutEffect(() => {
    topicGenerationRef.current += 1
    topicStoriesRef.current.clear()
    topicInflightRef.current.clear()
    const controller = new AbortController()
    readControllerRef.current = controller
    fetchLiveRef.current = true
    return () => {
      fetchLiveRef.current = false
      controller.abort()
      inflightRef.current.clear()
    }
  }, [board, stampKey])

  useEffect(() => {
    if (board !== 'feeds' || sources.length === 0) return
    for (const i of cover) {
      const source = sources[i]
      if (!source) continue
      const stamp = source.last_success_at ?? 0
      const key = `${source.id}:${stamp}`
      const exact = peekFeedStories(source.id, stamp)
      const slot = slotsRef.current.get(source.id)
      if (exact && slot?.stamp !== stamp) {
        slotsRef.current.set(source.id, { stamp, items: exact })
        bump()
      }
      if (exact || slot?.stamp === stamp) continue
      if (inflightRef.current.has(key)) continue
      inflightRef.current.add(key)
      const signal = readControllerRef.current.signal
      void loadFeedStories(source.id, stamp, signal)
        .then((items) => {
          if (signal.aborted) return
          inflightRef.current.delete(key)
          const current = sourcesRef.current.find((entry) => entry.id === source.id)
          if (!fetchLiveRef.current) return
          if ((current?.last_success_at ?? 0) !== stamp) return
          slotsRef.current.set(source.id, { stamp, items })
          bump()
        })
        .catch(() => {
          if (!signal.aborted) inflightRef.current.delete(key)
        })
    }
  }, [board, stampKey, cover, bump])

  const fetched = useMemo(() => {
    const map = new Map<number, FeedStory[]>()
    for (const source of sourcesRef.current) {
      const stamp = source.last_success_at ?? 0
      const exact = peekFeedStories(source.id, stamp)
      const slot = slotsRef.current.get(source.id)
      const items = exact ?? (slot?.stamp === stamp ? slot.items : null) ?? peekFeedStoriesLoose(source.id)
      if (items) map.set(source.id, items)
    }
    return map
    // stampKey：抓取戳变了才换表。tick：某源拉完后重拼。cover 只决定去拉谁。
  }, [stampKey, tick])

  const prevStoriesRef = useRef<FeedStory[]>([])
  const stories = useMemo(() => {
    const next =
      board === 'feeds'
        ? [
            ...latestFeedStories(sources, fetched),
            ...topicCards.flatMap((name, index) =>
              topicFeedStories(
                sources,
                fetched,
                name,
                topicFeedId(index),
                FEEDS_ARTICLE_MAX,
                topicStoriesRef.current.get(name),
              ),
            ),
            ...stitchStoriesBySources(sources, fetched, 0, lastIndex),
          ].map((story) => flags.project(story))
        : []
    const reused = reuseFeedStories(prevStoriesRef.current, next)
    prevStoriesRef.current = reused
    return reused
  }, [board, fetched, flags, flagsRevision, lastIndex, sources, topicCards])

  const onStar = useCallback(
    (item: PhantasiItemPreview) => onToggleStar?.(flags.project(item)),
    [onToggleStar, flags],
  )

  return {
    stories,
    onStar,
    jump,
    holdStories,
    releaseStories,
    railEpoch: idKey,
    topicCards,
  }
}
