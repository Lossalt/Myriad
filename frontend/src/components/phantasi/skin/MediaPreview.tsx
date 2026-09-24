import { displayImageUrl } from '../notes/noteImageUrl'

/** Thumbnail of a media asset at its permanent address (private ones use the session). */
export function MediaPreview({
  src,
  video = false,
  className,
}: {
  src?: string
  video?: boolean
  className?: string
}) {
  if (!src) return <span className={`${className ?? ''} is-empty`} />
  const shown = displayImageUrl(src)
  if (video) {
    return <video className={className} src={shown} muted playsInline preload="metadata" />
  }
  return <img className={className} src={shown} alt="" />
}
