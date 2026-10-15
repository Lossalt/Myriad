import type { AnnotationItem } from '../../../services/phantasiaiApi'
import type { TocItem } from '../reading/types'
import { useEffect } from 'react'
import { loadEmbedData } from '../../../utils/embedProcessor'

interface UseContentPostprocessOptions {
  contentRef: React.RefObject<HTMLDivElement | null>
  baseContent: string
  showAnnotations: boolean
  annotations: AnnotationItem[]
  comments: unknown
  theme: unknown
  setToc: (toc: TocItem[]) => void
}

export function useContentPostprocess({
  contentRef,
  baseContent,
  showAnnotations: _showAnnotations,
  annotations: _annotations,
  comments: _comments,
  theme: _theme,
  setToc,
}: UseContentPostprocessOptions): void {
  useEffect(() => {
    if (contentRef.current) {
      const headings = contentRef.current.querySelectorAll(
        'h1, h2, h3, h4, h5, h6',
      )
      const tocItems: TocItem[] = []

      headings.forEach((heading, index) => {
        if (heading.closest('.note-widget')) return
        const level = Number.parseInt(heading.tagName[1])
        const text = heading.textContent?.trim() || ''
        const id = `heading-${index}-${text.slice(0, 20).replaceAll(/\s+/g, '-').toLowerCase()}`

        heading.id = id

        if (text) {
          tocItems.push({ id, text, level })
        }
      })

      setToc(tocItems)

      const loadTimer = setTimeout(() => {
        if (contentRef.current) {
          loadEmbedData(contentRef.current).catch((err) => {
            console.error('[PhantasiReader] 加载嵌入数据失败:', err)
          })
        }
      }, 100)

      return () => clearTimeout(loadTimer)
    }
  }, [baseContent, contentRef, setToc])
}
