import assert from 'node:assert/strict'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const src = fileURLToPath(new URL('..', import.meta.url))

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) return sources(path)
    return /\.(?:ts|tsx)$/.test(name) && !/\.test\.tsx?$/.test(name) ? [path] : []
  })
}

test('cataloged events are only built by emitAppEvent', () => {
  const catalog = readFileSync(join(src, 'utils/appEvents.ts'), 'utf8')
  const names = [...catalog.matchAll(/^\s+'([^']+)':/gm)].map((match) => match[1])
  assert.ok(names.includes('wallpaperChanged') && names.includes('music-player-state-change'))
  const offenders: string[] = []
  for (const file of sources(src)) {
    if (file.endsWith('utils/appEvents.ts')) continue
    const text = readFileSync(file, 'utf8')
    for (const name of names) {
      if (text.includes(`new CustomEvent('${name}'`) || text.includes(`new CustomEvent("${name}"`)) {
        offenders.push(`${file.slice(src.length)}: ${name}`)
      }
    }
  }
  // A raw dispatch skips the payload type, which is how events drifted before.
  assert.deepEqual(offenders, [])
})
