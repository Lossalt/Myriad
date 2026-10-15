import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('source catalog abort contract', () => {
  it('getSources and getStats still accept an AbortSignal', () => {
    const api = readFileSync(join(dir, '../../services/phantasiApi.ts'), 'utf8')
    assert.match(
      api,
      /export async function getSources\([\s\S]*options\?: \{[\s\S]*view\?: 'catalog'[\s\S]*category\?: string[\s\S]*board\?: 'feeds' \| 'notes' \| 'sites'[\s\S]*forceRefresh\?: boolean[\s\S]*\}/,
    )
    assert.match(
      api,
      /export async function getStats\([\s\S]*options\?: \{ signal\?: AbortSignal; forceRefresh\?: boolean \}/,
    )
  })
})
