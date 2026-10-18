import assert from 'node:assert/strict'
import test from 'node:test'
import { BargeIn } from './bargeIn'

function harness(playing = true) {
  const calls: string[] = []
  const timers: (() => void)[] = []
  const bargeIn = new BargeIn(
    {
      hold: () => {
        calls.push('hold')
        return playing
      },
      resume: () => calls.push('resume'),
      cancel: () => calls.push('cancel'),
    },
    {
      setTimer: (run) => timers.push(run),
      clearTimer: () => {},
    },
  )
  return { bargeIn, calls, timers }
}

test('talking over her holds her voice, and meant speech cuts it off', () => {
  const { bargeIn, calls } = harness()
  bargeIn.begin()
  assert.equal(bargeIn.held, true)
  bargeIn.confirm()
  assert.equal(bargeIn.held, false)
  assert.deepEqual(calls, ['hold', 'cancel'])
})

test('a sound too short to be speech lets her go on', () => {
  const { bargeIn, calls } = harness()
  bargeIn.begin()
  bargeIn.dismiss()
  assert.deepEqual(calls, ['hold', 'resume'])
})

test('short speech decides by its words', () => {
  const worded = harness()
  worded.bargeIn.begin()
  worded.bargeIn.awaitWords()(true)
  assert.deepEqual(worded.calls, ['hold', 'cancel'])

  const wordless = harness()
  wordless.bargeIn.begin()
  wordless.bargeIn.awaitWords()(false)
  assert.deepEqual(wordless.calls, ['hold', 'resume'])
})

test('a slow transcription lets her go on, and late words still cut her off', () => {
  const { bargeIn, calls, timers } = harness()
  bargeIn.begin()
  const settle = bargeIn.awaitWords()
  timers.shift()!()
  assert.deepEqual(calls, ['hold', 'resume'])
  settle(true)
  assert.deepEqual(calls, ['hold', 'resume', 'cancel'])
})

test('only the latest speech decides', () => {
  const { bargeIn, calls } = harness()
  bargeIn.begin()
  const stale = bargeIn.awaitWords()
  bargeIn.begin()
  stale(false)
  assert.equal(bargeIn.held, true)
  assert.deepEqual(calls, ['hold'])
})

test('meant speech still cuts off a reply that had not been heard yet', () => {
  const { bargeIn, calls } = harness(false)
  bargeIn.begin()
  bargeIn.confirm()
  assert.deepEqual(calls, ['hold', 'cancel'])
})

test('stopping listening releases the hold and nothing decides later', () => {
  const { bargeIn, calls, timers } = harness()
  bargeIn.begin()
  const settle = bargeIn.awaitWords()
  bargeIn.reset()
  settle(true)
  for (const run of timers) run()
  assert.deepEqual(calls, ['hold', 'resume'])
})
