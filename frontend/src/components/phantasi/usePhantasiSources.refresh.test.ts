import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ApiError } from '../../services/api.ts'
import { isRefreshInProgress } from './usePhantasiSources.ts'

test('a refresh already in flight is not a failure', () => {
  assert.equal(isRefreshInProgress(new ApiError('Source is already being refreshed', 409, 'source_refresh_in_progress')), true)
  assert.equal(isRefreshInProgress(new ApiError('Failed to fetch feed', 502, 'unmapped')), false)
  assert.equal(isRefreshInProgress(new Error('x')), false)
})
