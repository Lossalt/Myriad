import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { ApiError } from '../../../services/api'
import { isSlugTakenError, noteSlugInvalid, noteSlugWire, publishedNoteHref } from './noteSlug.ts'

describe('noteSlugWire', () => {
  it('空串交给后端生成', () => {
    assert.equal(noteSlugWire(''), null)
    assert.equal(noteSlugWire('   '), null)
    assert.equal(noteSlugInvalid(''), false)
  })

  it('收成小写，只留字母数字和连字符', () => {
    assert.equal(noteSlugWire('Hello-World'), 'hello-world')
    assert.equal(noteSlugWire('n2'), 'n2')
    assert.equal(noteSlugWire('Hello World'), undefined)
    assert.equal(noteSlugWire('-hello'), undefined)
    assert.equal(noteSlugWire('hello-'), undefined)
    assert.equal(noteSlugInvalid('Hello World'), true)
    assert.equal(noteSlugWire('a'.repeat(80)), 'a'.repeat(80))
    assert.equal(noteSlugWire('a'.repeat(81)), undefined)
  })
})

describe('publishedNoteHref', () => {
  it('优先用响应里的 link，否则按 slug 拼博客地址', () => {
    assert.equal(publishedNoteHref({ link: '/blog/posts/hi/', slug: 'other' }), '/blog/posts/hi/')
    assert.equal(publishedNoteHref({ slug: 'hi' }), '/blog/posts/hi/')
    assert.equal(publishedNoteHref({ link: '  ' }), null)
    assert.equal(publishedNoteHref({}), null)
  })
})

describe('isSlugTakenError', () => {
  it('只把带 slug 的 409 当成地址占用', () => {
    assert.equal(isSlugTakenError(new ApiError('slug taken', 409, 'slug_taken')), true)
    assert.equal(isSlugTakenError(new ApiError('Slug already exists', 409)), true)
    assert.equal(isSlugTakenError(new ApiError('Note draft was updated elsewhere', 409)), false)
    assert.equal(isSlugTakenError(new ApiError('slug taken', 400, 'slug_taken')), false)
  })
})
