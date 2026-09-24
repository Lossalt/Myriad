import { authSubject, authSubjectKey } from './authSubject'
import { phantasiSubject, phantasiSubjectKey } from './phantasiSubject'
import { beginTappSubjectChange, finishTappSubjectChange } from './tappSubject'

interface IdentityUser {
  id: number
  is_admin?: boolean
  is_owner?: boolean
}

/**
 * The one place identity-scoped state learns that the signed-in subject is
 * changing. Every scope (shared request caches, Phantasi reads, TAPP runtime
 * consumers) is invalidated here, so a new change point cannot forget one of
 * them. Module state keyed to the subject registers its own cleanup through
 * `authSubject.subscribe`.
 *
 * Returns the TAPP subject epoch that `finishIdentityChange` must present.
 */
export function beginIdentityChange(): number {
  authSubject.change('changing', true)
  phantasiSubject.change('changing', false, true)
  return beginTappSubjectChange()
}

/** The confirmed subject after a change; `null` is a guest. */
export function settleIdentity(user: IdentityUser | null): void {
  if (user) {
    authSubject.change(authSubjectKey(user))
    phantasiSubject.change(phantasiSubjectKey(user))
  } else {
    authSubject.change('guest')
    phantasiSubject.change('guest')
  }
}

/**
 * TAPP consumers may resume once the old subject's runtime is torn down.
 * A superseded epoch is ignored, so a slow cleanup cannot announce a newer subject.
 */
export function finishIdentityChange(epoch: number, isAuthenticated: boolean): void {
  finishTappSubjectChange(epoch, isAuthenticated)
}
