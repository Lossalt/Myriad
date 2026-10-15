/** 工作台列表上的封面。概览页和笔记列表共用，不跟概览模块走。 */

export function WorkbenchThumb({
  src,
  video = false,
  cover = false,
}: {
  src?: string
  video?: boolean
  cover?: boolean
}) {
  const className = cover ? 'phantasi-workbench__cover' : 'phantasi-workbench__thumb'
  if (!src) return <span className={`${className} is-empty`} />
  if (video) {
    return (
      <video
        className={className}
        src={src}
        muted
        playsInline
        preload="metadata"
      />
    )
  }
  return <img className={className} src={src} alt="" />
}
