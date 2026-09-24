import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { currentCopy } from '../i18n/localeCopy.ts'
import { ApiError } from '../services/api.ts'
import { userFacingError } from './userFacingError.ts'

// Login / register / admin-user errors used to go through a separate regex
// classifier; they now resolve through the shared code table like every
// other surface, so the admin panel and the rest of the app show one copy.
describe('auth and admin-user errors resolve through byCode', () => {
  const byCode = () => currentCopy().errors.byCode as Record<string, string>

  // [backend `message`, backend `code`, backend `error` label]
  const cases: [string, string, string][] = [
    ['Username or password is incorrect', 'invalid_credentials', 'Invalid credentials'],
    [
      'This account has been linked to GitHub. Please use GitHub OAuth to login.',
      'local_login_disabled',
      'Local login disabled',
    ],
    [
      'Public registration is disabled. Ask an administrator to create an account.',
      'registration_disabled',
      'Registration disabled',
    ],
    ['Finish the setup wizard before creating an account.', 'setup_required', 'setup_required'],
    ['This username is already in use', 'username_taken', 'Username taken'],
    [
      'Keep a usable local login (password set and not disabled), or link another provider.',
      'last_sign_in_method',
      'Cannot unlink last identity',
    ],
    ["Cannot unlink the user's only sign-in method", 'last_sign_in_method', "Cannot unlink the user's only sign-in method"],
    ['Cannot delete your own account', 'self_delete_forbidden', 'Cannot delete your own account'],
    ['Cannot delete the last administrator', 'last_admin_required', 'Cannot delete the last administrator'],
    ['Cannot demote the last administrator', 'last_admin_required', 'Cannot demote the last administrator'],
    ['Cannot revoke your own admin role', 'self_demote_forbidden', 'Cannot revoke your own admin role'],
    [
      'Only the site owner can delete administrators',
      'admin_delete_owner_only',
      'Only the site owner can delete administrators',
    ],
    ['Only the site owner can change admin roles', 'admin_role_owner_only', 'Only the site owner can change admin roles'],
    ['Cannot delete the site owner', 'owner_delete_forbidden', 'Cannot delete the site owner'],
    ['Cannot demote the site owner', 'owner_demote_forbidden', 'Cannot demote the site owner'],
    [
      'Cannot disable Tapp install for the site owner',
      'owner_install_block_forbidden',
      'Cannot disable Tapp install for the site owner',
    ],
    [
      'Tapp installation is disabled for this account',
      'tapp_install_disabled',
      'Tapp installation is disabled for this account',
    ],
  ]

  // Messages registered as `leftovers` in shared/error_codes.json.
  const LEFTOVER_MESSAGES = new Set([
    'Username or password is incorrect',
    'Public registration is disabled. Ask an administrator to create an account.',
    'Finish the setup wizard before creating an account.',
    'This username is already in use',
  ])

  it('uses the code the backend sends', () => {
    for (const [message, code] of cases) {
      const copy = byCode()[code]
      assert.ok(copy, `byCode.${code} missing`)
      assert.equal(userFacingError(new ApiError(message, 400, code), 'fallback'), copy, code)
    }
  })

  it('still finds the copy from the bare label or message text', () => {
    for (const [message, code, label] of cases) {
      const copy = byCode()[code]
      // A code-shaped label (`setup_required`) arrives as `code` via parseApiErrorBody.
      if (label !== code) {
        assert.equal(userFacingError(new Error(label), 'fallback'), copy, label)
      }
      if (message === label || LEFTOVER_MESSAGES.has(message)) {
        assert.equal(userFacingError(new Error(message), 'fallback'), copy, message)
      }
    }
  })
})
