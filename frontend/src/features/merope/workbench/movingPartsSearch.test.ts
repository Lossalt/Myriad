import assert from 'node:assert/strict'
import { afterEach, it, mock } from 'node:test'
import { apiService } from '../../../services/api'
import { movingPartsSearching } from './movingPartsSearch'

afterEach(() => mock.restoreAll())

it('a search running on the server is found for its own figure only', async () => {
  mock.method(apiService, 'get', async () => ({ searches: [{ outfitId: null, seconds: 12 }, { outfitId: 'w-1', seconds: 3 }] }))
  assert.equal(await movingPartsSearching(), true, 'the worn bust')
  assert.equal(await movingPartsSearching('w-1'), true, 'that full-body set')
  assert.equal(await movingPartsSearching('w-2'), false, 'another set')
  mock.method(apiService, 'get', async () => ({ searches: [] }))
  assert.equal(await movingPartsSearching(), false, 'nothing runs')
})
