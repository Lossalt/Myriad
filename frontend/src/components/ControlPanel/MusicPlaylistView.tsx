import type { UseMusicPlayerReturn } from '../../hooks/useMusicPlayer'
import { LuSearchX } from '@lib/chromeStrokeIcons'
import { memo, useCallback, useEffect, useMemo } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { getSongVipStatus, highlightText } from '../../utils/musicPlayer'
import { PlayingSpectrum } from '../shared/PlayingSpectrum'
import { trackActivePlaylistItem } from './playlistAutoScroll'

const PlaylistItem = memo<{
  song: {
    id: string
    name: string
    artist: string
    isVip?: boolean
    vipType?: string
  }
  originalIndex: number
  isActive: boolean
  isPlaying: boolean
  useSpectrum: boolean
  searchQuery: string
  onSelect: (song: any, index: number, autoPlay: boolean) => void
  onClose: () => void
}>(
  ({
    song,
    originalIndex,
    isActive,
    isPlaying,
    useSpectrum,
    searchQuery,
    onSelect,
    onClose,
  }) => {
    const vipStatus = getSongVipStatus(song)

    const handleClick = useCallback(() => {
      onSelect(song, originalIndex, true)
      onClose()
    }, [song, originalIndex, onSelect, onClose])

    return (
      <div
        onClick={handleClick}
        className={`music-playlist-item ${isActive ? 'active' : ''}`}
      >
        <span className="music-playlist-index">{originalIndex + 1}</span>
        <div className="music-playlist-info">
          <div className="music-playlist-name-row">
            {/* 远端曲名不可信；无搜索词也必须走 highlightText，不得把原值塞进 innerHTML。 */}
            <div
              className="music-playlist-name"
              dangerouslySetInnerHTML={{
                __html: highlightText(song.name, searchQuery),
              }}
            />
            {vipStatus.displayText && (
              <span
                className={`music-vip-badge ${vipStatus.isTrial ? 'trial' : ''}`}
              >
                {vipStatus.displayText}
              </span>
            )}
          </div>
          <div
            className="music-playlist-artist"
            dangerouslySetInnerHTML={{
              __html: highlightText(song.artist, searchQuery),
            }}
          />
        </div>
        {isActive && (
          <span
            className={`music-playlist-playing${isPlaying ? ' is-playing' : ''}`}
            aria-hidden
          >
            <PlayingSpectrum
              themeColor="var(--music-base-primary, #ec4899)"
              scale={0.7}
              isPlaying={isPlaying}
              useSpectrum={useSpectrum}
            />
          </span>
        )}
      </div>
    )
  },
)

PlaylistItem.displayName = 'PlaylistItem'

/** visible=false 时 is-hidden 保 DOM（滚动位置/列表不卸载） */
export const MusicPlaylistView = memo<Pick<UseMusicPlayerReturn,
  | 'playlist' | 'currentSongIndex' | 'isPlaying' | 'playlistSearchQuery'
  | 'excludeVipSongs' | 'setMusicPlayerView' | 'setPlaylistSearchQuery'
  | 'setExcludeVipSongs' | 'selectSong' | 'playlistScrollRef'> & { visible: boolean }
>(({ visible, ...player }) => {
  const { t } = useI18n()
  const anim = useAnimationLevel()
  // 与小组件一致：仅 standard 级走实时频谱；隐藏时关掉
  const useSpectrum = visible && anim.level === 'standard'
  const {
    playlist,
    currentSongIndex,
    isPlaying,
    playlistSearchQuery,
    excludeVipSongs,
    setMusicPlayerView,
    setPlaylistSearchQuery,
    setExcludeVipSongs,
    selectSong,
    playlistScrollRef,
  } = player

  // Reconnect for the selected song or filtered list; geometry owns readiness.
  useEffect(() => {
    if (!visible || playlist.length === 0 || playlistSearchQuery.trim()) return
    const scroller = playlistScrollRef.current
    if (!scroller) return
    return trackActivePlaylistItem(scroller)
  }, [visible, playlist, currentSongIndex, playlistSearchQuery, excludeVipSongs, playlistScrollRef])

  // 预计算歌曲 ID 到索引的映射，避免 O(n²) 查找
  const songIdToIndex = useMemo(() => {
    const map = new Map<string, number>()
    playlist.forEach((song, index) => {
      map.set(song.id, index)
    })
    return map
  }, [playlist])

  const handleClosePlaylist = useCallback(() => {
    setMusicPlayerView('info')
    setPlaylistSearchQuery('')
  }, [setMusicPlayerView, setPlaylistSearchQuery])

  if (playlist.length === 0) {
    return null
  }

  const displayPlaylist = playlistSearchQuery.trim()
    ? playlist.filter((song) => {
        const query = playlistSearchQuery.toLowerCase()
        return (
          song.name.toLowerCase().includes(query) ||
          song.artist.toLowerCase().includes(query)
        )
      })
    : playlist

  return (
    <div
      className={`music-view music-view-playlist${visible ? '' : ' is-hidden'}`}
      aria-hidden={!visible}
      inert={!visible ? true : undefined}
    >
      <div className="music-playlist-header">
        <button
          type="button"
          onClick={() => {
            setMusicPlayerView('info')
            setPlaylistSearchQuery('')
          }}
          className="music-back-btn music-lyrics-back-btn"
          aria-label={t.music.back}
        >
          <svg
            className="music-lyrics-back-btn__icon"
            fill="currentColor"
            viewBox="0 0 20 20"
            aria-hidden
          >
            <path
              fillRule="evenodd"
              d="M12.707 5.293a1 1 0 010 1.414L9.414 10l3.293 3.293a1 1 0 01-1.414 1.414l-4-4a1 1 0 010-1.414l4-4a1 1 0 011.414 0z"
              clipRule="evenodd"
            />
          </svg>
          <span className="music-lyrics-back-btn__label">{t.music.back}</span>
        </button>
        <div className="music-playlist-title">
          <span className="music-playlist-title-text">
            {t.music.playlistTitle}
          </span>
          <span className="music-playlist-count">
            {displayPlaylist.length}/{playlist.length}
          </span>
        </div>

        <button
          type="button"
          onClick={() => setExcludeVipSongs(!excludeVipSongs)}
          className={`music-vip-filter-toggle ${!excludeVipSongs ? 'active' : ''}`}
          aria-label={
            excludeVipSongs ? t.music.showVipSongs : t.music.hideVipSongs
          }
          title={excludeVipSongs ? t.music.showVipSongs : t.music.hideVipSongs}
        >
          <svg fill="currentColor" viewBox="0 0 24 24" aria-hidden>
            <path d="M5 16L3 7l5.5 4L12 5l3.5 6L21 7l-2 9H5zm0 2h14v2H5v-2z" />
          </svg>
        </button>

        <div className="music-playlist-search-compact">
          <svg
            className="music-search-icon"
            fill="currentColor"
            viewBox="0 0 20 20"
            aria-hidden
          >
            <path
              fillRule="evenodd"
              d="M8 4a4 4 0 100 8 4 4 0 000-8zM2 8a6 6 0 1110.89 3.476l4.817 4.817a1 1 0 01-1.414 1.414l-4.816-4.816A6 6 0 012 8z"
              clipRule="evenodd"
            />
          </svg>
          <input
            type="text"
            placeholder={t.music.searchPlaceholder}
            value={playlistSearchQuery}
            onChange={(e) => setPlaylistSearchQuery(e.target.value)}
            className="music-search-input"
          />
          {playlistSearchQuery && (
            <button
              type="button"
              onClick={() => setPlaylistSearchQuery('')}
              className="music-search-clear"
              aria-label={t.music.clearSearch}
            >
              <svg fill="currentColor" viewBox="0 0 20 20" aria-hidden>
                <path
                  fillRule="evenodd"
                  d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z"
                  clipRule="evenodd"
                />
              </svg>
            </button>
          )}
        </div>
      </div>

      <div
        className="music-playlist-scroll"
        ref={playlistScrollRef}
      >
        {displayPlaylist.length > 0 ? (
          displayPlaylist.map((song, idx) => {
            const originalIndex =
              songIdToIndex.get(song.id) ?? idx

            return (
              <PlaylistItem
                key={song.id}
                song={song}
                originalIndex={originalIndex}
                isActive={currentSongIndex === originalIndex}
                isPlaying={
                  visible && currentSongIndex === originalIndex
                    ? isPlaying
                    : false
                }
                useSpectrum={useSpectrum}
                searchQuery={playlistSearchQuery}
                onSelect={selectSong}
                onClose={handleClosePlaylist}
              />
            )
          })
        ) : (
          <div className="music-no-results">
            <div className="music-no-results-icon">
              <LuSearchX size={20} />
            </div>
            <div className="music-no-results-text">{t.music.noMatching}</div>
          </div>
        )}
      </div>
    </div>
  )
})
