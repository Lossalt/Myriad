import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import {
  getPhantasiGuidesCatalog,
  loadPhantasiGuidesCatalog,
} from './catalog.ts'
import './assertCatalogs.ts'

describe('phantasi guide catalog', () => {
  it('does not statically import ja or zh catalogs', () => {
    const src = readFileSync(new URL('./catalog.ts', import.meta.url), 'utf8')
    assert.equal(src.includes("from './catalog.ja-JP.json'"), false)
    assert.equal(src.includes("from './catalog.zh-CN.json'"), false)
  })

  it('serves English synchronously', () => {
    const en = getPhantasiGuidesCatalog('en-US')
    assert.ok(en.overview.what.includes('Admin console'))
    assert.ok(en.notesIo.what)
    assert.ok(en.markdown.what)
  })

  it('loads Chinese on demand and then serves it synchronously', async () => {
    const zh = await loadPhantasiGuidesCatalog('zh-CN')
    assert.equal(getPhantasiGuidesCatalog('zh-CN'), zh)
    assert.ok(zh.overview.what.includes('站长后台'))
    assert.notEqual(
      zh.overview.what,
      getPhantasiGuidesCatalog('en-US').overview.what,
    )
  })
})
