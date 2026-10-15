import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  blogBuildPolls,
  blogBuildStatus,
  blogHugoVersionLabel,
  blogSnapshotFromValue,
  blogThemeCanUninstall,
  blogThemeInstallUrl,
  blogThemeRemovalReturnsToDefault,
  DEFAULT_BLOG_THEME_ID,
  parseBlogParams,
  sameBlogSettings,
} from './blogState.ts'

describe('blog build status', () => {
  it('只在排队和构建中轮询', () => {
    assert.equal(blogBuildPolls('queued'), true)
    assert.equal(blogBuildPolls('running'), true)
    for (const status of ['idle', 'ok', 'failed', 'unavailable'] as const) {
      assert.equal(blogBuildPolls(status), false)
    }
  })

  it('不认识的状态不当成在跑', () => {
    assert.equal(blogBuildStatus('nope'), 'idle')
    assert.equal(blogBuildPolls(blogBuildStatus('nope')), false)
    assert.equal(blogBuildStatus('failed'), 'failed')
  })
})

describe('blog theme rules', () => {
  it('内置主题不能卸载', () => {
    assert.equal(blogThemeCanUninstall({ builtin: true }), false)
    assert.equal(blogThemeCanUninstall({ id: 'paper', builtin: false }), true)
    assert.equal(blogThemeCanUninstall({ id: DEFAULT_BLOG_THEME_ID, builtin: false }), false)
  })

  it('卸掉当前主题会回到内置主题', () => {
    assert.equal(blogThemeRemovalReturnsToDefault('paper', 'paper'), true)
    assert.equal(blogThemeRemovalReturnsToDefault('paper', DEFAULT_BLOG_THEME_ID), false)
  })

  it('安装地址只收 http 和 https', () => {
    assert.equal(blogThemeInstallUrl('https://example.com/theme.zip'), 'https://example.com/theme.zip')
    assert.equal(blogThemeInstallUrl('javascript:alert(1)'), null)
    assert.equal(blogThemeInstallUrl('not a url'), null)
    assert.equal(blogThemeInstallUrl('  '), null)
  })
})

describe('blog params', () => {
  it('只接受对象，空文本是空对象', () => {
    assert.deepEqual(parseBlogParams(''), {})
    assert.deepEqual(parseBlogParams(' {"a":1} '), { a: 1 })
    assert.equal(parseBlogParams('[]'), null)
    assert.equal(parseBlogParams('"x"'), null)
    assert.equal(parseBlogParams('{'), null)
  })
})

describe('blogHugoVersionLabel', () => {
  it('只留版本号和 extended', () => {
    assert.equal(
      blogHugoVersionLabel('hugo v0.168.0-e27a6760c5547237f87d353aebe0580feadb20f4+extended darwin/arm64 BuildDate=2026-10-13T16:42:50Z VendorInfo=gohugoio'),
      'v0.168.0 extended',
    )
    assert.equal(blogHugoVersionLabel('hugo v0.140.2 linux/amd64'), 'v0.140.2')
    assert.equal(blogHugoVersionLabel(' dev build '), 'dev build')
  })
})

describe('blogSnapshotFromValue', () => {
  it('缺字段走默认，坏主题丢掉', () => {
    const snapshot = blogSnapshotFromValue({
      settings: { site_title: 'Notes' },
      themes: [{ id: 'myriad-default', builtin: true }, { name: 'nope' }, null],
      hugo: { available: false },
    })
    assert.equal(snapshot.settings.site_title, 'Notes')
    assert.equal(snapshot.settings.theme_id, '')
    assert.deepEqual(snapshot.settings.params, {})
    assert.deepEqual(snapshot.themes.map((theme) => theme.id), ['myriad-default'])
    assert.equal(snapshot.themes[0]?.builtin, true)
    assert.equal(snapshot.hugo.available, false)
    assert.equal(snapshot.build.status, 'idle')
    assert.equal(sameBlogSettings(snapshot.settings, {
      ...snapshot.settings,
      description: '',
    }), true)
  })
})
