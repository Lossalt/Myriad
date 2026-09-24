import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { FEDERATION_CONTENT_KINDS } from './federation.ts'

const SHARED = JSON.parse(
  readFileSync(new URL('../../../shared/federation_content_kinds.json', import.meta.url), 'utf8'),
) as string[]

describe('federation content kinds', () => {
  it('matches the backend ContentKind list', () => {
    assert.deepEqual([...FEDERATION_CONTENT_KINDS], SHARED)
  })
})
