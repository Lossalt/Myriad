import assert from 'node:assert/strict'
import test from 'node:test'
import { MouthTransitionController } from './mouthTransition'

const PROFILE = { version: 1, source: 'bounds-fallback', silhouettes: [], bridges: [] } as const
const SPEAKING = { mouthOpen: 0.5, mouthWide: 0, mouthRound: 0, mouthNarrow: 0, maniac: 0, mouthSeal: 0, mouthEase: 0 }

test('a passing consonant narrows the vowel art; a held narrow shape swaps its own in', () => {
  const mouth = new MouthTransitionController(PROFILE)
  assert.equal(mouth.sample(SPEAKING).material, 'mouthOpen')
  // A consonant between vowels leans toward narrow without owning the mouth.
  assert.equal(mouth.sample({ ...SPEAKING, mouthNarrow: 0.6 }).material, 'mouthOpen')
  assert.equal(mouth.sample(SPEAKING).material, 'mouthOpen')
  // Held clearly narrow, it does.
  assert.equal(mouth.sample({ ...SPEAKING, mouthNarrow: 0.95 }).material, 'mouthNarrow')
  // Leaving narrow for a vowel stays as quick as any other change.
  assert.equal(mouth.sample({ ...SPEAKING, mouthNarrow: 0.4 }).material, 'mouthOpen')
})

test('lips parting straight into a consonant use the narrow art', () => {
  const mouth = new MouthTransitionController(PROFILE)
  assert.equal(mouth.sample({ ...SPEAKING, mouthOpen: 0 }).material, 'mouthClose')
  assert.equal(mouth.sample({ ...SPEAKING, mouthNarrow: 1 }).material, 'mouthNarrow')
})
