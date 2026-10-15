import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('embed boundary', () => {
  it('不从阅读器、朋友们或创作台外壳进口', () => {
    const files = readdirSync(dir).filter((name) =>
      (name.endsWith('.ts') || name.endsWith('.tsx')) && !name.endsWith('.test.ts'),
    )
    const src = files.map((name) => readFileSync(join(dir, name), 'utf8')).join('\n')
    assert.doesNotMatch(src, /phantasi\/reader/)
    assert.doesNotMatch(src, /PhantasiFriends/)
    assert.doesNotMatch(src, /ApplyFriendMode/)
    assert.doesNotMatch(src, /views\/Phantasi/)
    assert.doesNotMatch(src, /views\/Studio/)
    assert.doesNotMatch(src, /NavigationIsland/)
  })
})
