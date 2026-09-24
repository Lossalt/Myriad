import { authSubject, authSubjectKey } from './authSubject'
import { phantasiSubject, phantasiSubjectKey } from './phantasiSubject'

interface IdentityUser {
  id: number
  is_admin?: boolean
  is_owner?: boolean
}

/**
 * The one place identity-scoped state learns that the signed-in subject is
 * changing. Every scope (shared request caches, Phantasi reads) aborts its
 * in-flight work here, so a new change point cannot forget one of them.
 */
export function beginIdentityChange(): void {
  authSubject.change('changing', true)
  phantasiSubject.change('changing', false, true)
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
