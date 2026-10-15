import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { pageIdFromPath } from '../../hooks/animation/pageId.ts'
import { resolvePageRouteAnimation } from '../../tapp/routing/tappRouteMeta.ts'
import { parseStudioPath, studioAccess, studioNotePath, studioSectionPath } from './studioRoutes.ts'

describe('parseStudioPath', () => {
  it('创作侧各有地址，订阅面板不在其中', () => {
    assert.deepEqual(parseStudioPath('/studio'), { kind: 'section', section: 'home' })
    assert.deepEqual(parseStudioPath('/studio/'), { kind: 'section', section: 'home' })
    assert.deepEqual(parseStudioPath('/studio/notes'), { kind: 'section', section: 'notes' })
    assert.deepEqual(parseStudioPath('/studio/notes/new'), { kind: 'editor', docId: 'new' })
    assert.deepEqual(parseStudioPath('/studio/notes/12'), { kind: 'editor', docId: 12 })
    assert.equal(parseStudioPath('/studio/notes/0'), null)
    assert.equal(parseStudioPath('/studio/notes/new/extra'), null)
    assert.deepEqual(parseStudioPath('/studio/comments'), { kind: 'section', section: 'comments' })
    assert.deepEqual(parseStudioPath('/studio/media'), { kind: 'section', section: 'media' })
    assert.deepEqual(parseStudioPath('/studio/reviews'), { kind: 'section', section: 'reviews' })
    assert.deepEqual(parseStudioPath('/studio/categories'), { kind: 'section', section: 'categories' })
    assert.deepEqual(parseStudioPath('/studio/import'), { kind: 'section', section: 'import' })
    assert.deepEqual(parseStudioPath('/studio/blog'), { kind: 'section', section: 'blog' })
    assert.equal(parseStudioPath('/studio/feeds'), null)
    assert.equal(parseStudioPath('/journal/workbench/notes'), null)
    assert.equal(studioSectionPath('home'), '/studio')
    assert.equal(studioSectionPath('import'), '/studio/import')
    assert.equal(studioNotePath('new'), '/studio/notes/new')
    assert.equal(studioNotePath(4), '/studio/notes/4')
  })

  it('非管理员去登录或回首页', () => {
    assert.equal(studioAccess({ hasChecked: false, isAuthenticated: false, isAdmin: false }), 'pending')
    assert.equal(studioAccess({ hasChecked: true, isAuthenticated: false, isAdmin: false }), 'login')
    assert.equal(studioAccess({ hasChecked: true, isAuthenticated: true, isAdmin: false }), 'home')
    assert.equal(studioAccess({ hasChecked: true, isAuthenticated: true, isAdmin: true }), 'allow')
  })

  it('子路径共用一页动画', () => {
    assert.equal(pageIdFromPath('/studio'), 'studio')
    assert.equal(pageIdFromPath('/studio/notes/new'), 'studio')
    assert.equal(resolvePageRouteAnimation('/studio').key, '/studio')
    assert.equal(resolvePageRouteAnimation('/studio/blog').key, '/studio')
  })
})
