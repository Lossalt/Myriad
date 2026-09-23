import assert from 'node:assert/strict'
import test from 'node:test'
import { startHomeDashboardLoad } from './homeDashboardLoader'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}
async function flush() { for (let i = 0; i < 20; i++) await Promise.resolve() }
function fixture() {
  const read = deferred<Record<string, unknown>>()
  const ready = deferred<void>()
  const events: string[] = []
  const fallback = { standard: [], free: [] }
  const generation = { current: 0 }
  const options = {
    read: () => read.promise,
    preload: () => ready.promise,
    fallback: () => fallback,
    generation,
    mode: (value: string) => { events.push(`mode:${value}`) },
    title: (value: string) => { events.push(`title:${value}`) },
    raw: () => { events.push('raw') },
    apply: () => { events.push('apply') },
    error: async () => 'failure',
    notify: (value: string) => { events.push(value) },
  }
  return { read, ready, events, generation, options }
}

test('disposed configuration requests cannot update mode, title, or layout', async () => {
  const f = fixture()
  const dispose = startHomeDashboardLoad(f.options)
  dispose()
  f.read.resolve({ dashboard_title: 'late', dashboard_layout_mode: 'free' })
  await flush()
  assert.deepEqual(f.events, [])
})

test('ready layouts apply once and their old deadline cannot replay them', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const f = fixture()
  const dispose = startHomeDashboardLoad(f.options)
  f.read.resolve({ dashboard_title: 'Home', dashboard_layout_mode: 'free' })
  await flush()
  assert.deepEqual(f.events, ['mode:free', 'title:Home'])
  f.ready.resolve()
  await flush()
  assert.equal(f.events.at(-1), 'apply')
  t.mock.timers.tick(5000)
  await flush()
  assert.equal(f.events.filter(e => e === 'apply').length, 1)
  dispose()
})

test('readiness has a 3 second cap, but newer layout generations win', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const f = fixture()
  const dispose = startHomeDashboardLoad(f.options)
  f.read.resolve({})
  await flush()
  t.mock.timers.tick(2999)
  await flush()
  assert.ok(!f.events.includes('apply'))
  t.mock.timers.tick(1)
  await flush()
  assert.ok(f.events.includes('apply'))
  dispose()
  const next = fixture()
  const stop = startHomeDashboardLoad(next.options)
  next.read.resolve({})
  await flush()
  next.generation.current++
  next.ready.resolve()
  await flush()
  assert.ok(!next.events.includes('apply'))
  stop()
})

test('unmount cancels waiting layout and late error presentation', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const f = fixture()
  const stop = startHomeDashboardLoad(f.options)
  f.read.resolve({})
  await flush()
  stop()
  f.ready.resolve()
  t.mock.timers.tick(5000)
  await flush()
  assert.ok(!f.events.includes('apply'))
  const next = fixture()
  const formatted = deferred<string>()
  const dispose = startHomeDashboardLoad({ ...next.options, error: () => formatted.promise })
  next.read.reject(new Error('network'))
  await flush()
  dispose()
  formatted.resolve('late error')
  await flush()
  assert.deepEqual(next.events, [])
})

test('configuration failure reports once and applies the default layout', async () => {
  const f = fixture()
  const stop = startHomeDashboardLoad(f.options)
  f.read.reject(new Error('network'))
  f.ready.resolve()
  await flush()
  assert.deepEqual(f.events, ['failure', 'mode:standard', 'title:Dashboard', 'apply'])
  stop()
})

test('preload learns the parsed mode, and a failed read preloads the standard fallback', async () => {
  const seen: string[] = []
  const f = fixture()
  const dispose = startHomeDashboardLoad({ ...f.options, preload: (_layouts, mode) => { seen.push(mode); return Promise.resolve() } })
  f.read.resolve({ dashboard_layout_mode: 'free' })
  await flush()
  dispose()
  const g = fixture()
  const stop = startHomeDashboardLoad({ ...g.options, preload: (_layouts, mode) => { seen.push(mode); return Promise.resolve() } })
  g.read.reject(new Error('network'))
  await flush()
  stop()
  assert.deepEqual(seen, ['free', 'standard'])
})
