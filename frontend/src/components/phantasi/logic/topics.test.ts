import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { topicDisplayName, topicHue, topicNameKey } from './topics.ts'

const LEFTOVER_KEYS = [
  ['engineering', 'topicEngineering'],
  ['systems', 'topicSystems'],
  ['ai', 'topicAi'],
  ['product', 'topicProduct'],
  ['writing', 'topicWriting'],
  ['tools', 'topicTools'],
  ['culture', 'topicCulture'],
  ['security', 'topicSecurity'],
  ['oss', 'topicOss'],
  ['hardware', 'topicHardware'],
] as const

describe('leftover topic keys', () => {
  it('旧英文 key 能翻译上色，自建名没有 nameKey', () => {
    for (const [key, nameKey] of LEFTOVER_KEYS) {
      assert.equal(topicNameKey(key), nameKey, `${key} 缺 nameKey`)
      assert.match(topicHue(key), /^#[0-9a-f]{6}$/i, `${key} 的 hue 不是 hex`)
    }
    assert.equal(topicNameKey('Rust'), null)
    assert.equal(topicNameKey('other'), null)
    assert.match(topicHue('Rust'), /^hsl\(/)
  })
})

describe('topicDisplayName', () => {
  it('旧 key 读 i18n，自建名原样显示', () => {
    assert.equal(
      topicDisplayName({ key: 'ai', nameKey: 'topicAi' }, { topicAi: '人工智能' }),
      '人工智能',
    )
    assert.equal(topicDisplayName({ key: 'ai' }, { topicAi: '人工智能' }), '人工智能')
    assert.equal(topicDisplayName({ key: 'ai', nameKey: 'topicAi' }, {}), 'ai')
    assert.equal(topicDisplayName({ key: 'Rust' }, { topicAi: '人工智能' }), 'Rust')
  })
})
