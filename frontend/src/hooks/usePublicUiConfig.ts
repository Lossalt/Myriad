import type { UIConfigChange } from '../utils/requestDedup'
import { useEffect } from 'react'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { useVisibleState } from './useVisibleState'

/** Share transport/cache, but let each mounted consumer own its latest reload. */
export function usePublicUiConfig<T = Record<string, unknown>>(changedEvent: UIConfigChange) {
  const [config, setConfig] = useVisibleState<T | null>(null)
  useEffect(() => {
    let active = true
    let revision = 0
    const reload = async () => {
      const request = ++revision
      try {
        const data = await getUIConfigDeduped()
        if (active && request === revision) setConfig(data as T)
      } catch {
        // A failed refresh should preserve the last usable presentation.
      }
    }
    const onChanged = () => { void reload() }
    window.addEventListener(changedEvent, onChanged)
    void reload()
    return () => {
      active = false
      window.removeEventListener(changedEvent, onChanged)
    }
  }, [changedEvent, setConfig])
  return config
}
