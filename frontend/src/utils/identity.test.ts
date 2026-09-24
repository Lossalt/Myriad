import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { authSubject } from './authSubject'
import { beginIdentityChange, settleIdentity } from './identity'
import { phantasiSubject } from './phantasiSubject'
import { getTappSubjectSnapshot } from './tappSubject'

test('one identity change moves every identity-scoped state together', () => {
  const auth = authSubject.revision
  const phantasi = phantasiSubject.getSnapshot().generation
  const tapp = getTappSubjectSnapshot().epoch
  const epoch = beginIdentityChange()
  assert.equal(phantasiSubject.getSnapshot().active, false)
  assert.equal(epoch, tapp + 1)
  assert.deepEqual(getTappSubjectSnapshot(), { epoch, ready: false })
  settleIdentity({ id: 7, is_admin: false })
  assert.ok(authSubject.revision >= auth + 2)
  assert.ok(phantasiSubject.getSnapshot().generation >= phantasi + 2)
  assert.equal(phantasiSubject.getSnapshot().active, true)
  settleIdentity(null)
  assert.equal(phantasiSubject.getSnapshot().key, 'guest')
})

test('the auth provider changes identity only through identity.ts', () => {
  const src = readFileSync(new URL('../contexts/AuthContext.tsx', import.meta.url), 'utf8')
  assert.doesNotMatch(src, /authSubject\.change\(|phantasiSubject\.change\(/)
  assert.doesNotMatch(src, /utils\/tappSubject'|TappSubjectChange\(/)
})
