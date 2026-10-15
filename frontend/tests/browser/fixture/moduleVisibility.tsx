import type { ModuleVisibilityLevel } from '../../../src/utils/moduleVisibility'
import { StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'
import apiService from '../../../src/services/api'
import {
  DEFAULT_MODULE_VISIBILITY_PREFERENCES,
  dispatchModuleVisibilityPreferencesUpdated,
  normalizeModuleVisibilityPreferences,
  useModuleVisibilityPreferences,
} from '../../../src/utils/moduleVisibility'

const requests: Array<(value: unknown) => void> = []
Object.assign(apiService, { get: () => new Promise(resolve => requests.push(resolve)) })
Object.assign(window, { moduleVisibilityFixture: {
  count: () => requests.length,
  finish: (index: number, library: string) => requests[index]({ preferences: { modules: { library } } }),
  update: (library: ModuleVisibilityLevel) => dispatchModuleVisibilityPreferencesUpdated(normalizeModuleVisibilityPreferences({ modules: { ...DEFAULT_MODULE_VISIBILITY_PREFERENCES.modules, library } })),
} })
function Consumer({ id }: { id: string }) {
  const { preferences, isLoading, reload } = useModuleVisibilityPreferences()
  return <section><output data-consumer={id}>{isLoading ? 'loading' : preferences.modules.library}</output><button onClick={() => void reload()}>Reload {id}</button></section>
}
function Fixture() {
  const [mounted, setMounted] = useState(true)
  return <><button onClick={() => setMounted(value => !value)}>Toggle consumers</button>{mounted && <><Consumer id="a" /><Consumer id="b" /></>}</>
}
createRoot(document.getElementById('root')!).render(<StrictMode><Fixture /></StrictMode>)
