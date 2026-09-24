import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { messageForOAuthError, sanitizeOAuthDesc } from './oauthErrorMessages'

const t = {
  auth: {
    oauthError: 'Failed ({code}). Retry.',
    oauthErrorWithDesc: '{message} Details: {desc}',
    oauthErrorStateMissing: 'state missing msg',
    oauthErrorStateExpired: 'state expired msg',
    oauthErrorStateReplay: 'state replay msg',
    oauthErrorStateSlugMismatch: 'slug mismatch msg',
    oauthErrorMissingCode: 'missing code msg',
    oauthErrorMissingState: 'missing state msg',
    oauthErrorAccessDenied: 'access denied msg',
    oauthErrorTemporarilyUnavailable: 'temp unavail msg',
    oauthErrorServerError: 'server error msg',
    oauthErrorInvalidRequest: 'invalid request msg',
    oauthErrorUnauthorizedClient: 'unauthorized client msg',
    oauthErrorUnsupportedResponseType: 'unsupported rt msg',
    oauthErrorInvalidScope: 'invalid scope msg',
    oauthErrorTokenExchange: 'token exchange msg',
    oauthErrorProfileFetch: 'profile fetch msg',
    oauthErrorProviderUnavailable: 'provider unavail msg',
    oauthErrorLoginFailed: 'login failed msg',
    oauthErrorEmailAlreadyRegistered: 'email already registered msg',
    oauthErrorBrowserTxMismatch: 'browser tx mismatch msg',
    oauthErrorLinkFailed: 'link failed msg',
  },
} as any

function format(
  template: string,
  params: Record<string, string | number>,
): string {
  return Object.entries(params).reduce(
    (s, [k, v]) => s.replaceAll(`{${k}}`, String(v)),
    template,
  )
}

describe('sanitizeOAuthDesc', () => {
  it('strips controls and truncates', () => {
    assert.equal(sanitizeOAuthDesc('  hello\x00world  '), 'hello world')
    assert.equal(sanitizeOAuthDesc(null), null)
    assert.ok((sanitizeOAuthDesc('a'.repeat(200)) ?? '').endsWith('…'))
  })
})

describe('messageForOAuthError', () => {
  it('maps known codes', () => {
    assert.equal(
      messageForOAuthError('state_expired', null, t, format),
      'state expired msg',
    )
    assert.equal(
      messageForOAuthError('state_replay', null, t, format),
      'state replay msg',
    )
    assert.equal(
      messageForOAuthError('access_denied', null, t, format),
      'access denied msg',
    )
    assert.equal(
      messageForOAuthError('token_exchange_failed', null, t, format),
      'token exchange msg',
    )
    assert.equal(
      messageForOAuthError('email_already_registered', null, t, format),
      'email already registered msg',
    )
    assert.equal(
      messageForOAuthError('browser_tx_mismatch', null, t, format),
      'browser tx mismatch msg',
    )
    assert.equal(
      messageForOAuthError('link_failed', null, t, format),
      'link failed msg',
    )
  })

  it('appends safe desc for provider errors', () => {
    const msg = messageForOAuthError(
      'access_denied',
      'User cancelled',
      t,
      format,
    )
    assert.match(msg, /access denied msg/)
    assert.match(msg, /User cancelled/)
  })

  it('does not append desc for state errors', () => {
    const msg = messageForOAuthError(
      'state_expired',
      'should not appear',
      t,
      format,
    )
    assert.equal(msg, 'state expired msg')
  })

  it('falls back for unknown codes', () => {
    assert.match(
      messageForOAuthError('weird_code', null, t, format),
      /weird_code/,
    )
  })
})
