import type { PlaygroundErrorCopy } from './playgroundErrorMessages'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { currentCopy } from '../../i18n/localeCopy'
import {
  mapPlaygroundGenerateError,
  mapPlaygroundRuntimeError,
} from './playgroundErrorMessages'

const copy: PlaygroundErrorCopy = {
  playgroundTimeoutHint: 'TIMEOUT',
  playgroundServerErrorHint: 'SERVER',
  playgroundGenerateFailed: 'GENERIC',
  playgroundNetworkHint: 'NETWORK',
  playgroundErrorDetail: 'Detail: {detail}',
  playgroundRuntimeError: 'Runtime: {message}',
}

function format(
  template: string,
  params: Record<string, string | number>,
): string {
  return Object.entries(params).reduce(
    (s, [k, v]) => s.replaceAll(`{${k}}`, String(v)),
    template,
  )
}

function byCode(code: string): string {
  return (currentCopy().errors.byCode as Record<string, string>)[code]
}

describe('mapPlaygroundGenerateError', () => {
  it('maps user cancel through the shared table', () => {
    assert.ok(byCode('playground_cancelled'))
    assert.equal(
      mapPlaygroundGenerateError('whatever', copy, { userCancelled: true, format }),
      byCode('playground_cancelled'),
    )
  })

  it('maps browser timeouts and network failures to Playground copy', () => {
    for (const raw of ['TimeoutError', 'AbortError', 'The operation was aborted due to timeout.']) {
      assert.equal(mapPlaygroundGenerateError(raw, copy, { format }), 'TIMEOUT', raw)
    }
    assert.equal(mapPlaygroundGenerateError('Failed to fetch', copy, { format }), 'NETWORK')
  })

  it('reads every playground_* code from byCode', () => {
    for (const code of [
      'playground_ai_unconfigured',
      'playground_ai_failed',
      'playground_agent_busy',
      'playground_auth_required',
      'playground_admin_required',
      'playground_rate_limited',
      'playground_cancelled',
      'playground_generate_failed',
    ]) {
      assert.ok(byCode(code), `byCode.${code} missing`)
      assert.equal(mapPlaygroundGenerateError('x', copy, { format, code }), byCode(code), code)
    }
  })

  it('does not read an upstream timeout inside an AI failure as a browser timeout', () => {
    assert.equal(
      mapPlaygroundGenerateError('Pro AI agent generation failed: upstream timeout', copy, {
        format,
        code: 'playground_ai_failed',
      }),
      byCode('playground_ai_failed'),
    )
  })

  it('keeps validation, payload and bad-request detail', () => {
    assert.equal(
      mapPlaygroundGenerateError(
        'Generated Tapp did not pass validation after 3 attempts: missing core.js',
        copy,
        { format, code: 'playground_validation_failed' },
      ),
      `${byCode('playground_validation_failed')}\nDetail: missing core.js`,
    )
    const tooLarge = mapPlaygroundGenerateError(
      'Playground request body exceeds 8000000 bytes',
      copy,
      { format, code: 'playground_payload_too_large' },
    )
    assert.ok(tooLarge.startsWith(byCode('playground_payload_too_large')))
    assert.ok(tooLarge.includes('Detail: Playground request body exceeds'))
    assert.equal(
      mapPlaygroundGenerateError('Instruction must contain 1-32680 characters', copy, {
        format,
        code: 'playground_bad_request',
      }),
      `${byCode('playground_bad_request')}\nDetail: Instruction must contain 1-32680 characters`,
    )
  })

  it('reads middleware responses by status as the Playground would', () => {
    const cases: [number, string, string][] = [
      [401, 'Please login before using administrator functions.', 'playground_auth_required'],
      [403, 'Administrator access required. Only current admin users can perform this action.', 'playground_admin_required'],
      [403, 'CSRF token missing', 'playground_auth_required'],
      [429, 'HTTP 429', 'playground_rate_limited'],
      [413, 'Payload Too Large', 'playground_payload_too_large'],
    ]
    for (const [status, raw, code] of cases) {
      assert.ok(
        mapPlaygroundGenerateError(raw, copy, { format, status }).startsWith(byCode(code)),
        `${status} ${raw}`,
      )
    }
    assert.ok(
      mapPlaygroundGenerateError('HTTP 503', copy, { format, status: 503 }).startsWith('SERVER'),
    )
  })

  it('keeps descriptive localized messages', () => {
    assert.equal(
      mapPlaygroundGenerateError('包校验失败：缺少 main.js', copy, { format }),
      '包校验失败：缺少 main.js',
    )
  })
})

describe('mapPlaygroundRuntimeError', () => {
  it('prefixes message', () => {
    assert.equal(
      mapPlaygroundRuntimeError('TypeError: x is not a function', copy, format),
      'Runtime: TypeError: x is not a function',
    )
  })
})
