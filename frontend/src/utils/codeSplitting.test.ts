import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  CRITICAL_PRELOAD_ROUTES,
  lazyWithPreload,
  preloadRoutes,
  routeComponents,
} from './codeSplitting.ts'

describe('CRITICAL_PRELOAD_ROUTES', () => {
  it('does not prefetch Config or instance-specific Tapp pages', () => {
    assert.deepEqual(Iterator.from(CRITICAL_PRELOAD_ROUTES).toArray(), [
      'library',
      'tapp',
      'tappStore',
    ])
    assert.ok(
      !(CRITICAL_PRELOAD_ROUTES as readonly string[]).includes('config'),
    )
  })
})

describe('speculative route loading', () => {
  it('shares concurrent imports and retries a failed preload', async () => {
    let calls = 0
    const module = { default: () => null }
    const component = lazyWithPreload(async () => {
      if (++calls === 1) throw new Error('offline')
      return module
    })
    const first = component.preload()
    assert.equal(component.preload(), first)
    await assert.rejects(first, /offline/)
    assert.equal(await component.preload(), module)
    assert.equal(await component.preload(), module)
    assert.equal(calls, 2)
  })

  it('cancels idle work and stops a running batch after its current import', async (t) => {
    let queued: (() => void) | undefined
    let cancelledId: number | undefined
    const previous = Object.getOwnPropertyDescriptor(globalThis, 'window')
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: {
        requestIdleCallback: (callback: () => void) => {
          queued = callback
          return 7
        },
        cancelIdleCallback: (id: number) => {
          cancelledId = id
        },
      },
    })
    t.after(() => {
      if (previous) Object.defineProperty(globalThis, 'window', previous)
      else Reflect.deleteProperty(globalThis, 'window')
    })
    preloadRoutes(['library'])()
    assert.equal(cancelledId, 7)

    let release!: () => void
    let started = 0
    let nextStarted = 0
    t.mock.method(routeComponents.library, 'preload', () => {
      started++
      return new Promise<void>((resolve) => {
        release = resolve
      })
    })
    t.mock.method(routeComponents.tapp, 'preload', async () => {
      nextStarted++
    })
    const cancel = preloadRoutes(['library', 'tapp'])
    queued!()
    assert.equal(started, 1)
    cancel()
    release()
    await new Promise((resolve) => setImmediate(resolve))
    assert.equal(nextStarted, 0)
  })

  it('contains failures and continues the batch using the timer fallback', async (t) => {
    const previous = Object.getOwnPropertyDescriptor(globalThis, 'window')
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: { setTimeout, clearTimeout },
    })
    t.after(() => {
      if (previous) Object.defineProperty(globalThis, 'window', previous)
      else Reflect.deleteProperty(globalThis, 'window')
    })
    let complete!: () => void
    const finished = new Promise<void>((resolve) => {
      complete = resolve
    })
    t.mock.method(routeComponents.library, 'preload', async () => {
      throw new Error('offline')
    })
    t.mock.method(routeComponents.tapp, 'preload', async () => {
      complete()
    })
    const cancel = preloadRoutes(['library', 'tapp'])
    await finished
    cancel()
  })
})

describe('lazyWithPreload rendering', () => {
  it('renders an already loaded module in the same pass instead of a fallback', async () => {
    const { createElement, Suspense } = await import('react')
    const { renderToString } = await import('react-dom/server')
    const render = (component: Parameters<typeof createElement>[0]) =>
      renderToString(createElement(Suspense, { fallback: 'fallback' }, createElement(component)))

    const cold = lazyWithPreload(async () => ({ default: () => 'content' }))
    assert.match(render(cold), /fallback/)

    // A fallback commit would be revealed through React's ~300ms throttle.
    const warm = lazyWithPreload(async () => ({ default: () => 'content' }))
    await warm.preload()
    const html = render(warm)
    assert.match(html, /content/)
    assert.doesNotMatch(html, /fallback/)
  })
})
