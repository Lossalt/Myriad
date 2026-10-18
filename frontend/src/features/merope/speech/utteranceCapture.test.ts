import assert from 'node:assert/strict'
import test from 'node:test'
import {
  ENDPOINT_CEILING_SECONDS,
  ENDPOINT_FLOOR_SECONDS,
  UtteranceCapture,
} from './utteranceCapture'

function feed(
  capture: UtteranceCapture,
  ms: number,
  energy: number,
  size: number,
  playback = false,
) {
  const events: ReturnType<UtteranceCapture['push']>[] = []
  let remaining = Math.round((ms * capture.sampleRate) / 1000)
  while (remaining > 0) {
    const n = Math.min(size, remaining)
    events.push(capture.push(new Float32Array(n).fill(energy), playback))
    remaining -= n
  }
  return events
}

for (const size of [128, 320, 512]) {
  test(`endpoint uses milliseconds, preserves pre-roll and sentence pauses (${size} samples)`, () => {
    const capture = new UtteranceCapture(16_000)
    feed(capture, 250, 0.001, size)
    assert.equal(
      feed(capture, 240, 0.08, size).filter((e) => e.started).length,
      1,
    )
    assert.equal(
      feed(capture, 350, 0.001, size).some((e) => e.ended),
      false,
    )
    feed(capture, 240, 0.08, size)
    const ended = feed(capture, 600, 0.001, size).filter((e) => e.ended)
    assert.equal(ended.length, 1)
    const pcm = ended[0]!.utterance!.pcm
    assert.ok(
      pcm[0]![0]! < 0.01,
      'the lead-in before the opening gate is retained',
    )
    assert.ok(pcm.reduce((n, frame) => n + frame.length, 0) > 16_000)
  })
}

test('brief clicks and playback leakage do not trigger an interruption', () => {
  const capture = new UtteranceCapture(16_000)
  assert.equal(
    feed(capture, 40, 0.3, 128).some((e) => e.started),
    false,
  )
  feed(capture, 600, 0, 128)
  assert.equal(
    feed(capture, 500, 0.1, 128, true).some((e) => e.started),
    false,
  )
  assert.equal(
    feed(capture, 200, 0.2, 128, true).filter((e) => e.started).length,
    1,
  )
})

test('sustained input is bounded, reset never carries PCM into the next listener', () => {
  const capture = new UtteranceCapture(16_000)
  const ended = feed(capture, 21_000, 0.1, 128).filter((e) => e.ended)
  assert.equal(ended.length, 1)
  assert.ok(
    ended[0]!.utterance!.pcm.reduce((n, frame) => n + frame.length, 0) <=
      320_128,
  )
  capture.reset()
  assert.equal(capture.finish(), undefined)
  assert.equal(
    feed(capture, 700, 0, 128).some((e) => e.ended),
    false,
  )
})

test('a loud interruption can continue quietly after playback stops', () => {
  const capture = new UtteranceCapture(16_000)
  assert.equal(
    feed(capture, 200, 0.2, 128, true).some((event) => event.started),
    true,
  )
  assert.equal(
    feed(capture, 900, 0.025, 128).some((event) => event.ended),
    false,
  )
  assert.equal(
    feed(capture, 600, 0, 128).filter((event) => event.utterance).length,
    1,
  )
})

test('speech is confirmed as meant once, after half a second of voice', () => {
  const capture = new UtteranceCapture(16_000)
  const opening = feed(capture, 200, 0.2, 128, true)
  assert.equal(opening.some((event) => event.started), true)
  assert.equal(opening.some((event) => event.confirmed), false)
  const meant = feed(capture, 400, 0.2, 128, true)
  assert.equal(meant.filter((event) => event.confirmed).length, 1)
  const ended = feed(capture, 600, 0, 128)
  assert.equal(ended.some((event) => event.confirmed), false)
  assert.equal(ended.filter((event) => event.utterance).length, 1)
})

test('a short sound starts an utterance without confirming it', () => {
  const capture = new UtteranceCapture(16_000)
  const events = [
    ...feed(capture, 250, 0.2, 128, true),
    ...feed(capture, 600, 0, 128),
  ]
  assert.equal(events.filter((event) => event.started).length, 1)
  assert.equal(events.some((event) => event.confirmed), false)
  assert.equal(events.filter((event) => event.ended).length, 1)
})

/** Quiet until the utterance ends, then this much more before speaking again. */
function endThenWait(capture: UtteranceCapture, ms: number) {
  let ended = 0
  for (let i = 0; i < 200 && ended === 0; i += 1)
    ended += feed(capture, 10, 0.001, 160).filter((e) => e.utterance).length
  assert.equal(ended, 1)
  feed(capture, ms, 0.001, 160)
}

test('someone who picks a sentence up again right after it ended is given longer pauses', () => {
  const capture = new UtteranceCapture(16_000)
  assert.equal(capture.endpointDelay, ENDPOINT_FLOOR_SECONDS)
  for (let i = 0; i < 12; i += 1) {
    feed(capture, 300, 0.08, 160)
    endThenWait(capture, 250)
  }
  const learned = capture.endpointDelay
  assert.ok(learned > ENDPOINT_FLOOR_SECONDS + 0.1, `learned ${learned}`)
  assert.ok(learned <= ENDPOINT_CEILING_SECONDS)

  // The pause that used to split the sentence no longer does.
  feed(capture, 300, 0.08, 160)
  assert.equal(feed(capture, 600, 0.001, 160).some((e) => e.ended), false)
})

test('short pauses inside sentences bring the endpoint back toward the floor', () => {
  const capture = new UtteranceCapture(16_000)
  for (let i = 0; i < 6; i += 1) {
    feed(capture, 300, 0.08, 160)
    endThenWait(capture, 250)
  }
  const raised = capture.endpointDelay
  feed(capture, 300, 0.08, 160)
  for (let i = 0; i < 20; i += 1) {
    feed(capture, 300, 0.001, 160)
    feed(capture, 300, 0.08, 160)
  }
  assert.ok(capture.endpointDelay < raised)
  assert.ok(capture.endpointDelay >= ENDPOINT_FLOOR_SECONDS)
})

test('a long silence after a sentence is not a pause to learn from', () => {
  const capture = new UtteranceCapture(16_000)
  for (let i = 0; i < 6; i += 1) {
    feed(capture, 300, 0.08, 160)
    endThenWait(capture, 1_500)
  }
  assert.equal(capture.endpointDelay, ENDPOINT_FLOOR_SECONDS)
})

test('a reply in between is a new turn, not a pause to learn from', () => {
  const capture = new UtteranceCapture(16_000)
  for (let i = 0; i < 6; i += 1) {
    feed(capture, 300, 0.08, 160)
    endThenWait(capture, 0)
    feed(capture, 100, 0.001, 160, true)
    feed(capture, 150, 0.001, 160)
  }
  assert.equal(capture.endpointDelay, ENDPOINT_FLOOR_SECONDS)
})
