import type { MobileReaderBarProps } from '../reading/types'

import {
  LuAlignJustify as AlignJustify,
  LuArrowRight as ArrowRight,
  LuChevronLeft as ChevronLeft,
  LuChevronRight as ChevronRight,
  LuChevronUp as ChevronUp,
  LuCloud as Cloud,
  LuEdit3 as Edit3,
  LuExternalLink as ExternalLink,
  LuEye as Eye,
  LuEyeOff as EyeOff,
  LuList as List,
  LuMessageSquare as MessageSquare,
  LuMic as Mic,
  LuMinus as Minus,
  LuMonitor as Monitor,
  LuPalette as Palette,
  LuPause as Pause,
  LuPlay as Play,
  LuPlus as Plus,
  LuRefreshCw as RefreshCw,
  LuSkipBack as SkipBack,
  LuSkipForward as SkipForward,
  LuSparkles as Sparkles,
  LuSquare as Square,
  LuStar as Star,
  LuType as Type,
  LuX as X,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { memo } from 'react'

import { Spinner } from '../../Spinner'
import { annotationChrome } from './annotationChrome'
import {
  READER_COMMENTS_PANEL_ID,
  READER_MOBILE_CONTROLS_ID,
  READER_TOOL_SHEET_ID,
  READER_TOOL_TITLE_ID,
  STYLE_MAX_HEIGHT_60VH,
  THEMES,
} from '../reading/constants'
import {
  applyExclusivePanel,
  currentReaderPanel,
  readerDialogTrigger,
  readerPopupTrigger,
} from './readerPanels'
import { ReaderProgressPercent } from './ReaderProgress'
import { useReaderDialogFocus } from './useReaderDialogFocus'

export const MobileReaderBar = memo(
  ({
    item,
    onClose,
    onToggleStar,
    isAuthenticated,
    isAdmin,
    isPhantasiai,
    theme,
    currentTheme,
    isDark,
    readingProgress,
    showPanels,
    showMobileControls,
    setShowMobileControls,
    toc,
    showToc,
    setShowToc,
    activeHeadingId,
    scrollToHeading,
    comments,
    commentsEnabled,
    hasComments,
    showCommentsPanel,
    setShowCommentsPanel,
    annotations,
    annotationsLoading,
    showAnnotations,
    showPhantasiaiPanel,
    setShowPhantasiaiPanel,
    toggleAnnotations,
    loadAnnotations,
    regenerateAnnotations,
    annotationsError,
    selectedAnnotation,
    setSelectedAnnotation,
    scrollToAnnotation,
    podcastDialogues,
    podcastLoading,
    cloudTtsLoading,
    podcastState,
    showPodcastPlayer,
    setShowPodcastPlayer,
    loadPodcast,
    podcastCurrentIndex,
    ttsEngine,
    handleTtsEngineChange,
    cloudTtsAvailable,
    cloudTtsLoadProgress,
    handlePlayPause,
    handleStop,
    handlePrevious,
    handleNext,
    handleDialogueClick,
    cycleTheme,
    cycleFont,
    fontSize,
    adjustFontSize,
    lineHeight,
    adjustLineHeight,
    currentFont,
    handleShare,
    onEditNote,
    enableAnimations,
    onTouchStart,
    onTouchEnd,
    t,
  }: MobileReaderBarProps) => {
    const toolPanel = currentReaderPanel({
      toc: showToc,
      phantasiai: showPhantasiaiPanel,
      podcast: showPodcastPlayer,
    })
    const commentsOn = showCommentsPanel
    const activePanel = toolPanel
    const sheetOpen = activePanel !== null
    const closeRef = useReaderDialogFocus(sheetOpen, READER_TOOL_SHEET_ID)

    const openPanel = (
      panel: 'toc' | 'annotations' | 'podcast' | 'comments',
    ) => {
      if (panel === 'comments') {
        const next = !showCommentsPanel
        setShowCommentsPanel(next)
        if (next) {
          setShowToc(false)
          setShowPhantasiaiPanel(false)
          setShowPodcastPlayer(false)
        }
      } else {
        setShowCommentsPanel(false)
        const next = applyExclusivePanel(toolPanel, panel)
        setShowToc(next.toc)
        setShowPhantasiaiPanel(next.phantasiai)
        setShowPodcastPlayer(next.podcast)
      }
      setShowMobileControls(false)
    }

    const closePanel = () => {
      setShowCommentsPanel(false)
      setShowToc(false)
      setShowPhantasiaiPanel(false)
      setShowPodcastPlayer(false)
    }

    const minTocLevel =
      toc.length > 0 ? Math.min(...toc.map((t) => t.level)) : 1

    return (
      <>
        <div
          className="sm:hidden fixed bottom-0 left-0 right-0 z-30"
          onTouchStart={onTouchStart}
          onTouchEnd={onTouchEnd}
        >
          <AnimatePresence>
            {showPanels && (
              <motion.div
                initial={enableAnimations ? { opacity: 0, y: 24 } : false}
                animate={enableAnimations ? { opacity: 1, y: 0 } : undefined}
                exit={enableAnimations ? { opacity: 0, y: 24 } : undefined}
                transition={
                  enableAnimations
                    ? { duration: 0.3, ease: [0.16, 1, 0.3, 1] }
                    : undefined
                }
                className={`flex flex-col-reverse border-t ${currentTheme.border} ${currentTheme.surface}`}
              >
                <div className="flex items-center justify-between px-3 py-2 safe-area-inset-bottom">
                  <div className="flex items-center gap-2">
                    <button
                      onClick={onClose}
                      className="phantasi-reader__btn"
                      title={t.phantasi.backEsc}
                    >
                      <ChevronLeft className="w-5 h-5" />
                    </button>
                    {item.source_icon && (
                      <img
                        src={item.source_icon || undefined}
                        alt=""
                        className="w-6 h-6 rounded-lg"
                      />
                    )}
                    <span
                      className={`text-sm font-medium ${currentTheme.text} truncate max-w-25`}
                    >
                      {item.source_name}
                    </span>
                  </div>

                  <div className="flex items-center gap-2">
                    <ReaderProgressPercent
                      progress={readingProgress}
                      className={`text-xs font-medium ${currentTheme.secondary} tabular-nums`}
                    />
                    <button
                      onClick={() => setShowMobileControls(!showMobileControls)}
                      className={`phantasi-reader__btn${showMobileControls ? ' is-on' : ''}`}
                      title={t.phantasi.moreOptions}
                      {...readerPopupTrigger(
                        showMobileControls,
                        READER_MOBILE_CONTROLS_ID,
                      )}
                    >
                      <ChevronUp
                        className={`w-5 h-5 transition-transform ${showMobileControls ? 'rotate-180' : ''}`}
                      />
                    </button>
                  </div>
                </div>

                <AnimatePresence>
                  {showMobileControls && (
                    <motion.div
                      id={READER_MOBILE_CONTROLS_ID}
                      role="region"
                      aria-label={t.phantasi.moreOptions}
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: 'auto', opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      transition={{ duration: 0.25, ease: [0.16, 1, 0.3, 1] }}
                      className={`overflow-hidden border-b ${currentTheme.border}`}
                    >
                      <div className="px-3 py-3 space-y-3">
                        <div className="flex items-center justify-between">
                          <div className="flex items-center gap-1">
                            {toc.length > 0 && (
                              <button
                                onClick={() => openPanel('toc')}
                                className={`phantasi-reader__btn${activePanel === 'toc' ? ' is-on' : ''}`}
                                title={t.phantasi.tableOfContents}
                                {...readerDialogTrigger(
                                  activePanel === 'toc',
                                  READER_TOOL_SHEET_ID,
                                )}
                              >
                                <List className="w-5 h-5" />
                              </button>
                            )}

                            {isAuthenticated && isAdmin && (
                              <button
                                onClick={onToggleStar}
                                className={`phantasi-reader__btn${item.is_starred ? ' is-star' : ''}`}
                                title={
                                  item.is_starred
                                    ? t.phantasi.unstar
                                    : t.phantasi.starred
                                }
                              >
                                <Star
                                  className={`w-5 h-5 ${item.is_starred ? 'fill-current' : ''}`}
                                />
                              </button>
                            )}

                            {commentsEnabled ? (
                              <button
                                onClick={() => openPanel('comments')}
                                className={`phantasi-reader__btn relative${commentsOn ? ' is-on' : ''}`}
                                {...readerDialogTrigger(
                                  commentsOn,
                                  READER_COMMENTS_PANEL_ID,
                                )}
                                title={
                                  hasComments
                                    ? `${t.phantasi.viewComments} (${comments.length})`
                                    : isAuthenticated
                                      ? t.phantasi.selectTextToComment
                                      : t.phantasi.comment
                                }
                              >
                                <MessageSquare className="w-5 h-5" />
                                {hasComments && (
                                  <span className="absolute -top-0.5 -right-0.5 w-4 h-4 bg-amber-500 text-white text-[10px] font-bold rounded-full flex items-center justify-center">
                                    {comments.length > 9
                                      ? '9+'
                                      : comments.length}
                                  </span>
                                )}
                              </button>
                            ) : null}

                            {isPhantasiai &&
                              (isAdmin || item.has_ai_annotations) && (
                                <button
                                  onClick={() => openPanel('annotations')}
                                  {...readerDialogTrigger(
                                    activePanel === 'annotations',
                                    READER_TOOL_SHEET_ID,
                                  )}
                                  disabled={annotationsLoading}
                                  className={`p-2 rounded-xl ${
                                    activePanel === 'annotations' ||
                                    (showAnnotations && annotations.length > 0)
                                      ? 'text-purple-500 bg-purple-500/10'
                                      : `${currentTheme.secondary} hover:text-purple-500`
                                  }`}
                                  title={
                                    annotationsLoading
                                      ? `${t.phantasi.loading}...`
                                      : t.phantasi.aiAnnotations
                                  }
                                >
                                  {annotationsLoading ? (
                                    <Spinner size="sm" color="current" />
                                  ) : (
                                    <Sparkles className="w-5 h-5" />
                                  )}
                                </button>
                              )}

                            {isPhantasiai && (isAdmin || item.has_ai_podcast) && (
                              <button
                                onClick={() => {
                                  if (podcastDialogues.length === 0) {
                                    loadPodcast()
                                  }
                                  openPanel('podcast')
                                }}
                                {...readerDialogTrigger(
                                  activePanel === 'podcast',
                                  READER_TOOL_SHEET_ID,
                                )}
                                disabled={podcastLoading || cloudTtsLoading}
                                className={`p-2 rounded-xl ${
                                  podcastState === 'playing'
                                    ? 'text-emerald-500 bg-emerald-500/10 animate-pulse'
                                    : activePanel === 'podcast'
                                      ? 'text-emerald-500 bg-emerald-500/10'
                                      : `${currentTheme.secondary} hover:text-emerald-500`
                                }`}
                                title={
                                  podcastLoading
                                    ? t.phantasi.generatingPodcast
                                    : t.phantasi.aiPodcast
                                }
                              >
                                {podcastLoading || cloudTtsLoading ? (
                                  <Spinner size="sm" color="current" />
                                ) : (
                                  <Mic className="w-5 h-5" />
                                )}
                              </button>
                            )}

                            <a
                              href={item.link}
                              target="_blank"
                              rel="noopener noreferrer"
                              className="phantasi-reader__btn"
                              title={t.phantasi.readOriginal}
                            >
                              <ExternalLink className="w-5 h-5" />
                            </a>
                          </div>

                          <div className="flex items-center gap-1">
                            {onEditNote && (
                              <button
                                onClick={onEditNote}
                                className="phantasi-reader__btn"
                                title={t.phantasi.noteEdit}
                              >
                                <Edit3 className="w-5 h-5" />
                              </button>
                            )}

                          <button
                            onClick={handleShare}
                            className="phantasi-reader__btn"
                            title={t.phantasi.share}
                          >
                            <svg
                              className="w-5 h-5"
                              viewBox="0 0 24 24"
                              fill="none"
                              stroke="currentColor"
                              strokeWidth="2"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            >
                              <path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8" />
                              <polyline points="16 6 12 2 8 6" />
                              <line x1="12" x2="12" y1="2" y2="15" />
                            </svg>
                          </button>
                          </div>
                        </div>

                        <div className="flex items-center justify-between">
                          <button
                            onClick={cycleTheme}
                            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg ${isDark ? 'bg-white/5' : 'bg-black/5'} ${currentTheme.secondary}`}
                            title={t.phantasi.switchTheme}
                          >
                            <Palette className="w-4 h-4" />
                            <span className="text-xs">
                              {THEMES[theme].icon}
                            </span>
                          </button>

                          <button
                            onClick={cycleFont}
                            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg ${isDark ? 'bg-white/5' : 'bg-black/5'} ${currentTheme.secondary}`}
                            style={{ fontFamily: currentFont.family }}
                            title={t.phantasi[currentFont.labelKey]}
                          >
                            <Type className="w-4 h-4" />
                            <span className="text-xs">{t.phantasi.fontLabel}</span>
                          </button>

                          <div
                            className={`flex items-center gap-1 px-2 py-1 rounded-lg ${isDark ? 'bg-white/5' : 'bg-black/5'}`}
                          >
                            <button
                              onClick={() => adjustFontSize(-1)}
                              className={`p-1 rounded ${currentTheme.secondary} hover:${currentTheme.text}`}
                              title={t.phantasi.decreaseFontSize}
                            >
                              <Minus className="w-4 h-4" />
                            </button>
                            <span
                              className={`text-xs ${currentTheme.secondary} tabular-nums min-w-6 text-center`}
                            >
                              {fontSize}
                            </span>
                            <button
                              onClick={() => adjustFontSize(1)}
                              className={`p-1 rounded ${currentTheme.secondary} hover:${currentTheme.text}`}
                              title={t.phantasi.increaseFontSize}
                            >
                              <Plus className="w-4 h-4" />
                            </button>
                          </div>

                          <div
                            className={`flex items-center gap-1 px-2 py-1 rounded-lg ${isDark ? 'bg-white/5' : 'bg-black/5'}`}
                          >
                            <button
                              onClick={() => adjustLineHeight(-0.1)}
                              className={`p-1 rounded ${currentTheme.secondary} hover:${currentTheme.text}`}
                              title={t.phantasi.decreaseLineHeight}
                            >
                              <AlignJustify className="w-4 h-4 opacity-50" />
                            </button>
                            <span
                              className={`text-xs ${currentTheme.secondary} tabular-nums min-w-7 text-center`}
                            >
                              {lineHeight.toFixed(1)}
                            </span>
                            <button
                              onClick={() => adjustLineHeight(0.1)}
                              className={`p-1 rounded ${currentTheme.secondary} hover:${currentTheme.text}`}
                              title={t.phantasi.increaseLineHeight}
                            >
                              <AlignJustify className="w-4 h-4" />
                            </button>
                          </div>
                        </div>
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        <AnimatePresence>
          {activePanel && (
            <motion.div
              initial={enableAnimations ? { opacity: 0 } : false}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: enableAnimations ? 0.2 : 0, ease: 'easeOut' }}
              className="sm:hidden fixed inset-0 z-40 flex flex-col"
            >
              <div
                className="absolute inset-0 bg-black/50"
                onClick={closePanel}
              />

              <motion.div
                initial={enableAnimations ? { y: '100%' } : false}
                animate={{ y: 0 }}
                exit={{ y: '100%' }}
                transition={{ duration: enableAnimations ? 0.35 : 0, ease: [0.16, 1, 0.3, 1] }}
                id={READER_TOOL_SHEET_ID}
                data-phantasi-shortcuts="suspended"
                role="dialog"
                aria-modal="true"
                aria-labelledby={READER_TOOL_TITLE_ID}
                className={`absolute bottom-0 left-0 right-0 rounded-t-2xl ${currentTheme.surfaceSolid} border-t ${currentTheme.border} overflow-hidden`}
                style={STYLE_MAX_HEIGHT_60VH}
              >
                <div
                  className={`flex items-center justify-between px-4 py-3 border-b ${currentTheme.border}`}
                >
                  <div className="flex items-center gap-2">
                    {activePanel === 'toc' && (
                      <>
                        <List className="w-5 h-5 text-amber-500" />
                        <span
                          id={READER_TOOL_TITLE_ID}
                          className={`font-medium ${currentTheme.text}`}
                        >
                          {t.phantasi.tocTitle}
                        </span>
                        <span className={`text-sm ${currentTheme.secondary}`}>
                          ({toc.length})
                        </span>
                      </>
                    )}
                    {activePanel === 'annotations' && (
                      <>
                        <Sparkles className="w-5 h-5 text-purple-500" />
                        <span
                          id={READER_TOOL_TITLE_ID}
                          className={`font-medium ${currentTheme.text}`}
                        >
                          {t.phantasi.aiAnnotations}
                        </span>
                        {annotations.length > 0 && (
                          <span className={`text-sm ${currentTheme.secondary}`}>
                            ({annotations.length})
                          </span>
                        )}
                      </>
                    )}
                    {activePanel === 'podcast' && (
                      <>
                        <Mic className="w-5 h-5 text-emerald-500" />
                        <span
                          id={READER_TOOL_TITLE_ID}
                          className={`font-medium ${currentTheme.text}`}
                        >
                          {t.phantasi.aiPodcast}
                        </span>
                        {podcastDialogues.length > 0 && (
                          <span className={`text-sm ${currentTheme.secondary}`}>
                            {podcastCurrentIndex + 1}/{podcastDialogues.length}
                          </span>
                        )}
                      </>
                    )}
                  </div>
                  <button
                    ref={closeRef}
                    onClick={closePanel}
                    className="phantasi-reader__btn"
                    title={t.phantasi.close}
                  >
                    <X className="w-5 h-5" />
                  </button>
                </div>

                <div
                  className="overflow-y-auto"
                  style={{ maxHeight: 'calc(60vh - 60px)' }}
                >
                  {activePanel === 'toc' && (
                    <nav className="p-3 space-y-0.5">
                      {toc.map((item) => {
                        const isActive = item.id === activeHeadingId
                        const indent = (item.level - minTocLevel) * 12

                        return (
                          <button
                            key={item.id}
                            onClick={() => {
                              scrollToHeading(item.id)
                              closePanel()
                            }}
                            className={`w-full text-left px-3 py-2 rounded-xl text-sm transition-all duration-200 ease-out ${
                              isActive
                                ? `${isDark ? 'bg-white/10' : 'bg-black/5'} ${currentTheme.text} font-medium`
                                : `${currentTheme.secondary} hover:${currentTheme.text} ${isDark ? 'hover:bg-white/5' : 'hover:bg-black/3'}`
                            }`}
                            style={{ paddingLeft: `${12 + indent}px` }}
                          >
                            {isActive && (
                              <ChevronRight className="w-3 h-3 inline-block mr-1 -ml-1" />
                            )}
                            {item.text}
                          </button>
                        )
                      })}
                    </nav>
                  )}

                  {activePanel === 'annotations' && (
                    <div className="p-3">
                      <div className="flex items-center justify-between mb-3">
                        <button
                          onClick={toggleAnnotations}
                          className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm transition-all duration-200 ease-out ${
                            showAnnotations
                              ? 'text-purple-500 bg-purple-500/10'
                              : `${currentTheme.secondary} ${isDark ? 'bg-white/5' : 'bg-black/5'}`
                          }`}
                        >
                          {showAnnotations ? (
                            <Eye className="w-4 h-4" />
                          ) : (
                            <EyeOff className="w-4 h-4" />
                          )}
                          {showAnnotations
                            ? t.phantasi.hideHighlight
                            : t.phantasi.showAnnotations}
                        </button>
                        {isAdmin && (
                          <button
                            onClick={regenerateAnnotations}
                            disabled={annotationsLoading}
                            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm ${currentTheme.secondary} ${isDark ? 'bg-white/5' : 'bg-black/5'} disabled:opacity-50`}
                          >
                            {annotationsLoading ? (
                              <Spinner size="sm" color="current" />
                            ) : (
                              <RefreshCw className="w-4 h-4" />
                            )}
                            {t.phantasi.regenerate}
                          </button>
                        )}
                      </div>

                      {annotations.length === 0 ? (
                        <div
                          className={`py-8 text-center ${currentTheme.secondary}`}
                        >
                          {annotationsLoading ? (
                            <div className="flex flex-col items-center gap-2">
                              <Spinner size="lg" className="text-purple-500" />
                              <p className="text-sm">{t.phantasi.analyzing}</p>
                            </div>
                          ) : (
                            <div className="flex flex-col items-center gap-2">
                              <Sparkles className="w-8 h-8 opacity-30" />
                              <p className="text-sm">{t.phantasi.noAnnotations}</p>
                              {isAdmin && (
                                <button
                                  onClick={loadAnnotations}
                                  className="text-sm text-purple-500 hover:text-purple-600 font-medium"
                                >
                                  {t.phantasi.regenerate}
                                </button>
                              )}
                            </div>
                          )}
                        </div>
                      ) : (
                        <div className="space-y-2">
                          {annotations.map((annotation, index) => {
                            const typeConfig = annotationChrome(annotation.type)
                            const isSelected =
                              selectedAnnotation?.term === annotation.term

                            return (
                              <button
                                key={annotation.id || index}
                                onClick={() => {
                                  setSelectedAnnotation(
                                    isSelected ? null : annotation,
                                  )
                                  scrollToAnnotation(annotation)
                                  closePanel()
                                }}
                                className={`w-full text-left p-3 rounded-xl transition-all duration-200 ease-out ${
                                  isSelected
                                    ? `${typeConfig.bgColor} ${currentTheme.text}`
                                    : `${isDark ? 'bg-white/5 hover:bg-white/10' : 'bg-black/2 hover:bg-black/5'}`
                                }`}
                              >
                                <div className="flex items-center gap-2 mb-1 min-w-0">
                                  <span
                                    className={`text-xs px-1.5 py-0.5 rounded ${typeConfig.bgColor} ${typeConfig.color} shrink-0 whitespace-nowrap`}
                                  >
                                    {typeConfig.label}
                                  </span>
                                  <span
                                    className={`text-sm font-medium ${currentTheme.text} min-w-0 flex-1 truncate`}
                                  >
                                    {annotation.term}
                                  </span>
                                  <ArrowRight
                                    className={`w-3 h-3 ${currentTheme.secondary} shrink-0`}
                                  />
                                </div>
                                <p
                                  className={`text-xs ${currentTheme.secondary} leading-relaxed break-words [overflow-wrap:anywhere] ${isSelected ? '' : 'line-clamp-2'}`}
                                >
                                  {annotation.explanation}
                                </p>
                              </button>
                            )
                          })}
                        </div>
                      )}

                      {annotationsError && (
                        <div className="mt-3 px-3 py-2 text-sm text-red-500 bg-red-500/10 rounded-lg">
                          {annotationsError}
                        </div>
                      )}
                    </div>
                  )}

                  {activePanel === 'podcast' && (
                    <div className="p-3">
                      <div className="flex items-center justify-between mb-3">
                        <div className="flex items-center gap-2">
                          <button
                            onClick={() => handleTtsEngineChange('system')}
                            disabled={cloudTtsLoading}
                            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm transition-colors ${
                              ttsEngine === 'system'
                                ? 'bg-emerald-500/20 text-emerald-500'
                                : `${currentTheme.secondary} ${isDark ? 'bg-white/5' : 'bg-black/5'}`
                            } disabled:opacity-50`}
                          >
                            <Monitor className="w-4 h-4" />
                            {t.phantasi.systemTts}
                          </button>
                          <button
                            onClick={() => handleTtsEngineChange('cloud')}
                            disabled={cloudTtsLoading}
                            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm transition-colors ${
                              ttsEngine === 'cloud'
                                ? 'bg-emerald-500/20 text-emerald-500'
                                : cloudTtsAvailable
                                  ? `${currentTheme.secondary} ${isDark ? 'bg-white/5' : 'bg-black/5'}`
                                  : `${currentTheme.secondary} ${isDark ? 'bg-white/5' : 'bg-black/5'} opacity-60`
                            } disabled:opacity-50`}
                          >
                            {/* 加载环由下方状态行独担，按钮不再转圈。 */}
                            <Cloud className="w-4 h-4" />
                            {t.phantasi.cloudTts}
                          </button>
                        </div>
                      </div>

                      {cloudTtsLoading && (
                        <div
                          className={`mb-3 px-3 py-2 text-sm ${currentTheme.secondary} bg-emerald-500/10 rounded-lg flex items-center gap-2`}
                        >
                          <Spinner size="sm" color="current" />
                          <span>
                            {t.phantasi.loadingCloudVoice}{' '}
                            {cloudTtsLoadProgress.loaded}/
                            {cloudTtsLoadProgress.total}
                          </span>
                        </div>
                      )}

                      {podcastDialogues.length > 0 && (
                        <div
                          className={`flex items-center justify-center gap-4 mb-4 p-3 rounded-xl ${isDark ? 'bg-white/5' : 'bg-black/2'}`}
                        >
                          <button
                            onClick={handlePrevious}
                            disabled={podcastCurrentIndex === 0}
                            className={`p-2 rounded-xl ${currentTheme.secondary} hover:${currentTheme.text} disabled:opacity-30`}
                            title={t.phantasi.previousDialogue}
                          >
                            <SkipBack className="w-5 h-5" />
                          </button>
                          <button
                            onClick={handlePlayPause}
                            className={`p-4 rounded-full ${podcastState === 'playing' ? 'bg-emerald-500 text-white' : `${isDark ? 'bg-white/10' : 'bg-black/5'} ${currentTheme.text}`}`}
                            title={
                              podcastState === 'playing'
                                ? t.phantasi.pause
                                : t.phantasi.play
                            }
                          >
                            {podcastState === 'playing' ? (
                              <Pause className="w-6 h-6" />
                            ) : (
                              <Play className="w-6 h-6" />
                            )}
                          </button>
                          <button
                            onClick={handleNext}
                            disabled={
                              podcastCurrentIndex >= podcastDialogues.length - 1
                            }
                            className={`p-2 rounded-xl ${currentTheme.secondary} hover:${currentTheme.text} disabled:opacity-30`}
                            title={t.phantasi.nextDialogue}
                          >
                            <SkipForward className="w-5 h-5" />
                          </button>
                          <button
                            onClick={handleStop}
                            className={`p-2 rounded-xl ${currentTheme.secondary} hover:${currentTheme.text}`}
                            title={t.phantasi.stop}
                          >
                            <Square className="w-5 h-5" />
                          </button>
                        </div>
                      )}

                      {podcastDialogues.length === 0 ? (
                        <div
                          className={`py-8 text-center ${currentTheme.secondary}`}
                        >
                          {podcastLoading ? (
                            <div
                              className="flex flex-col items-center gap-2"
                              role="status"
                            >
                              <Spinner size="lg" className="text-emerald-500" />
                            </div>
                          ) : (
                            <div className="flex flex-col items-center gap-2">
                              <Mic className="w-8 h-8 opacity-30" />
                              <p className="text-sm">
                                {t.phantasi.noPodcast}
                              </p>
                              {isAdmin && (
                                <button
                                  onClick={loadPodcast}
                                  className="text-sm text-emerald-500 hover:text-emerald-600 font-medium"
                                >
                                  {t.phantasi.generatePodcast}
                                </button>
                              )}
                            </div>
                          )}
                        </div>
                      ) : (
                        <div className="space-y-2">
                          {podcastDialogues.map((dialogue, index) => {
                            const isHost = dialogue.speaker === 'host_a'
                            const isCurrent = index === podcastCurrentIndex
                            const isPlaying =
                              isCurrent && podcastState === 'playing'

                            return (
                              <button
                                key={index}
                                onClick={() => handleDialogueClick(index)}
                                className={`w-full text-left p-3 rounded-xl transition-all duration-200 ease-out ${
                                  isCurrent
                                    ? isPlaying
                                      ? 'bg-emerald-500/20 ring-2 ring-emerald-500/50'
                                      : `${isDark ? 'bg-white/10' : 'bg-black/5'}`
                                    : `${isDark ? 'hover:bg-white/5' : 'hover:bg-black/2'}`
                                }`}
                              >
                                <div className="flex items-start gap-2">
                                  <span
                                    className={`shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-xs font-medium ${
                                      isHost
                                        ? 'bg-blue-500/20 text-blue-500'
                                        : 'bg-pink-500/20 text-pink-500'
                                    }`}
                                  >
                                    {isHost
                                      ? t.phantasi.podcastHostLabel
                                      : t.phantasi.podcastGuestLabel}
                                  </span>
                                  <p
                                    className={`text-sm ${isCurrent ? currentTheme.text : currentTheme.secondary} leading-relaxed`}
                                  >
                                    {dialogue.text}
                                  </p>
                                </div>
                              </button>
                            )
                          })}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              </motion.div>
            </motion.div>
          )}
        </AnimatePresence>
      </>
    )
  },
)
