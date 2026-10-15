export function phantasiMotionQuiet(): boolean {
  if (typeof window === 'undefined') return true
  return (
    window.matchMedia('(prefers-reduced-motion: reduce)').matches ||
    document.documentElement.getAttribute('data-perf-mode') === 'exlight'
  )
}
