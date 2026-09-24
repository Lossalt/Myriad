import assert from 'node:assert/strict'
import test from 'node:test'
import { TappLoadGenerations } from './tappLoadGenerations'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

test('a superseded update load that resolves last is discarded', async () => {
  const generations = new TappLoadGenerations()
  const first = deferred<string>()
  const second = deferred<string>()
  const firstLoad = generations.settle('app', generations.begin('app'), first.promise)
  const secondLoad = generations.settle('app', generations.begin('app'), second.promise)

  second.resolve('v2')
  assert.deepEqual(await secondLoad, { stale: false, value: 'v2' })
  first.resolve('v1')
  assert.deepEqual(await firstLoad, { stale: true })
})

test('a superseded load failure is discarded instead of surfacing an error', async () => {
  const generations = new TappLoadGenerations()
  const first = deferred<string>()
  const firstLoad = generations.settle('app', generations.begin('app'), first.promise)
  generations.begin('app')
  first.reject(new Error('old bundle gone'))
  assert.deepEqual(await firstLoad, { stale: true })
})

test('the current load still reports its failure', async () => {
  const generations = new TappLoadGenerations()
  await assert.rejects(
    generations.settle('app', generations.begin('app'), Promise.reject(new Error('boom'))),
    /boom/,
  )
})

test('an open yields to an update that began after it, without superseding other apps', async () => {
  const generations = new TappLoadGenerations()
  const open = generations.settle('app', generations.current('app'), Promise.resolve('opened'))
  const other = generations.settle('other', generations.current('other'), Promise.resolve('other'))
  generations.begin('app')
  assert.deepEqual(await open, { stale: true })
  assert.deepEqual(await other, { stale: false, value: 'other' })
})
