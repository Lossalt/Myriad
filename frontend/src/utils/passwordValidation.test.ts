import assert from 'node:assert/strict'
import { it } from 'node:test'
import { loadLocale } from '../i18n/loadLocale'
import { ApiError } from '../services/api.ts'
import { passwordValidationCode } from './passwordValidation.ts'
import { userFacingError } from './userFacingError.ts'

it('shows localized password requirements for admin creation instead of HTTP 400', async () => {
  const t = await loadLocale('en-US')
  for (const [code, expected] of [
    ['password_too_short', t.errors.passwordMinLength],
    ['password_too_long', t.auth.passwordLengthError],
    ['password_needs_letter_and_digit', t.userModal.passwordNeedsLetterAndDigit],
  ]) {
    assert.equal(userFacingError(new ApiError('Invalid password', 400, code), 'fallback'), expected)
  }
})

it('validates Unicode scalar length and letter/number categories consistently with the backend', () => {
  for (const [password, expected] of [
    ['abc1234', 'password_too_short'],
    ['a1😀😀😀😀😀', 'password_too_short'],
    [`a1${'x'.repeat(127)}`, 'password_too_long'],
    ['abcdefgh', 'password_needs_letter_and_digit'],
    ['12345678', 'password_needs_letter_and_digit'],
    ['abc12345', undefined],
    [`a1${'😀'.repeat(126)}`, undefined],
    ['汉字かな۱۲۳۴', undefined],
    ['aⅣ😀😀😀😀😀😀', undefined],
  ] as const) {
    assert.equal(passwordValidationCode(password), expected)
  }
})
