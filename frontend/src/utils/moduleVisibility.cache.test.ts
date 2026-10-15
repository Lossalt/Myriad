/**
 * @vitest-environment node
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import apiService from '../services/api'
import {
  areModuleVisibilityPreferencesEqual,
  DEFAULT_MODULE_VISIBILITY_PREFERENCES,
  dispatchModuleVisibilityPreferencesUpdated,
  ensureModuleVisibilityPreferences,
  fetchModuleVisibilityPreferences,
  getCachedModuleVisibilityPreferences,
  normalizeModuleVisibilityPreferences,
  updateModuleVisibilityPreferences,
} from './moduleVisibility'

describe('module visibility session cache', () => {
  it('dispatchModuleVisibilityPreferencesUpdated warms getCachedModuleVisibilityPreferences', () => {
    const next = normalizeModuleVisibilityPreferences({
      modules: {
        ...DEFAULT_MODULE_VISIBILITY_PREFERENCES.modules,
        tapp: 'authenticated',
      },
    })
    dispatchModuleVisibilityPreferencesUpdated(next)
    const cached = getCachedModuleVisibilityPreferences()
    assert.ok(cached)
    assert.equal(cached!.modules.tapp, 'authenticated')
  })

  it('later dispatch overwrites prior cache (no stale admin-only leak)', () => {
    dispatchModuleVisibilityPreferencesUpdated(
      normalizeModuleVisibilityPreferences({
        modules: {
          ...DEFAULT_MODULE_VISIBILITY_PREFERENCES.modules,
          library: 'admin',
        },
      }),
    )
    dispatchModuleVisibilityPreferencesUpdated(
      normalizeModuleVisibilityPreferences({
        modules: {
          ...DEFAULT_MODULE_VISIBILITY_PREFERENCES.modules,
          library: 'all',
        },
      }),
    )
    assert.equal(getCachedModuleVisibilityPreferences()?.modules.library, 'all')
  })
})

function withLibrary(library: 'all' | 'authenticated' | 'admin') {
  return normalizeModuleVisibilityPreferences({
    modules: { ...DEFAULT_MODULE_VISIBILITY_PREFERENCES.modules, library },
  })
}

it('saves visibility through the shared API and a fresh visitor reads it', async (t) => {
  const saved = normalizeModuleVisibilityPreferences({
    modules: { ...DEFAULT_MODULE_VISIBILITY_PREFERENCES.modules, library: 'admin', phantasi: 'authenticated' },
  })
  let stored = DEFAULT_MODULE_VISIBILITY_PREFERENCES
  t.mock.method(apiService, 'put', async (url: string, payload: typeof saved) => {
    assert.equal(url, '/config/module-visibility')
    stored = structuredClone(payload)
    return { success: true, preferences: stored }
  })
  t.mock.method(apiService, 'get', async (url: string) => {
    assert.equal(url, '/config/module-visibility')
    return { success: true, preferences: structuredClone(stored) }
  })
  await updateModuleVisibilityPreferences(saved)
  // Discard this browser's cached state before the visitor fetch.
  dispatchModuleVisibilityPreferencesUpdated(DEFAULT_MODULE_VISIBILITY_PREFERENCES)
  const visitor = await fetchModuleVisibilityPreferences()
  assert.deepEqual(visitor, saved)
  assert.equal(areModuleVisibilityPreferencesEqual(visitor, DEFAULT_MODULE_VISIBILITY_PREFERENCES), false)
})

it('failed saves leave the last confirmed visibility in place', async (t) => {
  dispatchModuleVisibilityPreferencesUpdated(withLibrary('authenticated'))
  t.mock.method(apiService, 'put', async () => { throw new Error('offline') })
  await assert.rejects(updateModuleVisibilityPreferences(withLibrary('admin')), /offline/)
  assert.equal(getCachedModuleVisibilityPreferences()?.modules.library, 'authenticated')
})

it('an authoritative preference update invalidates older session loads', async t => {
  const held = Promise.withResolvers<{ preferences: typeof DEFAULT_MODULE_VISIBILITY_PREFERENCES }>()
  t.mock.method(apiService, 'get', () => held.promise)
  const old = ensureModuleVisibilityPreferences(true)
  const saved = withLibrary('admin')
  dispatchModuleVisibilityPreferencesUpdated(saved)
  held.resolve({ preferences: DEFAULT_MODULE_VISIBILITY_PREFERENCES })
  assert.deepEqual(await old, saved)
  assert.deepEqual(getCachedModuleVisibilityPreferences(), saved)
})

it('a forced session refresh starts a fresh transport and owns the final snapshot', async t => {
  const old = Promise.withResolvers<{ preferences: typeof DEFAULT_MODULE_VISIBILITY_PREFERENCES }>()
  const fresh = Promise.withResolvers<{ preferences: typeof DEFAULT_MODULE_VISIBILITY_PREFERENCES }>()
  let calls = 0
  t.mock.method(apiService, 'get', () => ++calls === 1 ? old.promise : fresh.promise)
  const first = ensureModuleVisibilityPreferences(true)
  const second = ensureModuleVisibilityPreferences(true)
  const saved = withLibrary('admin')
  fresh.resolve({ preferences: saved })
  assert.deepEqual(await second, saved)
  old.resolve({ preferences: DEFAULT_MODULE_VISIBILITY_PREFERENCES })
  assert.deepEqual(await first, saved)
  assert.equal(calls, 2)
  assert.deepEqual(getCachedModuleVisibilityPreferences(), saved)
})

it('a successful save supersedes an already pending session read', async t => {
  const held = Promise.withResolvers<{ preferences: typeof DEFAULT_MODULE_VISIBILITY_PREFERENCES }>()
  const saved = withLibrary('authenticated')
  t.mock.method(apiService, 'get', () => held.promise)
  t.mock.method(apiService, 'put', async () => ({ success: true, preferences: saved }))
  const old = ensureModuleVisibilityPreferences(true)
  await updateModuleVisibilityPreferences(saved)
  held.resolve({ preferences: DEFAULT_MODULE_VISIBILITY_PREFERENCES })
  assert.deepEqual(await old, saved)
  assert.deepEqual(getCachedModuleVisibilityPreferences(), saved)
})
