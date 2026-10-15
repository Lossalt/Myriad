import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { articleReturnPath, loginHref, safeSameOriginPath } from './safeReturnPath.ts'

const origin = 'https://notes.example'

describe('safeSameOriginPath', () => {
  it('只留本站路径', () => {
    assert.equal(safeSameOriginPath('/blog/posts/hi/', origin), '/blog/posts/hi/')
    assert.equal(
      safeSameOriginPath('https://notes.example/blog/posts/hi/?x=1#c', origin),
      '/blog/posts/hi/?x=1#c',
    )
    assert.equal(safeSameOriginPath('https://evil.example/blog/', origin), null)
    assert.equal(safeSameOriginPath('//evil.example/phish', origin), null)
    assert.equal(safeSameOriginPath('javascript:alert(1)', origin), null)
    assert.equal(safeSameOriginPath('/login?return=/', origin), null)
    assert.equal(safeSameOriginPath('  ', origin), null)
  })
})

describe('articleReturnPath', () => {
  it('query 优先，然后父页，再然后 referrer；嵌入地址不当回跳', () => {
    assert.equal(articleReturnPath({
      origin,
      search: '?return=/blog/posts/a/',
      referrer: 'https://notes.example/blog/posts/b/',
      parentHref: 'https://notes.example/blog/posts/c/',
    }), '/blog/posts/a/')
    assert.equal(articleReturnPath({
      origin,
      search: '',
      referrer: 'https://notes.example/blog/posts/b/',
      parentHref: 'https://notes.example/blog/posts/c/',
    }), '/blog/posts/c/')
    assert.equal(articleReturnPath({
      origin,
      search: '',
      referrer: 'https://notes.example/blog/posts/b/',
    }), '/blog/posts/b/')
    assert.equal(articleReturnPath({
      origin,
      search: '',
      referrer: 'https://notes.example/embed/comments/3',
      parentHref: 'https://notes.example/embed/comments/3',
    }), '/blog/')
    assert.equal(articleReturnPath({
      origin,
      search: '?return=https://evil.example/x',
      referrer: 'https://evil.example/y',
    }), '/blog/')
  })

  it('登录链接把回跳放进 query', () => {
    assert.equal(loginHref('/blog/posts/hi/'), '/login?return=%2Fblog%2Fposts%2Fhi%2F')
    assert.equal(loginHref(null), '/login')
  })
})
