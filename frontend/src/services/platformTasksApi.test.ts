import assert from 'node:assert/strict'
import { afterEach, it, mock } from 'node:test'
import { apiService } from './api'
import { getRunningPlatformTask } from './platformTasksApi'

afterEach(() => mock.restoreAll())

const task = (status: string) => ({ success: true, task: { id: 't1', platform: 'steam', status, progress: 0.4, created_at: '', updated_at: '' } })

it('a platform task still running is found again, a finished one is not', async () => {
  const get = mock.method(apiService, 'get', async () => task('Processing'))
  assert.equal(await getRunningPlatformTask('steam'), 't1')
  assert.equal(get.mock.calls[0].arguments[0], '/tasks/platform/steam')
  mock.method(apiService, 'get', async () => task('Pending'))
  assert.equal(await getRunningPlatformTask('steam'), 't1')
  mock.method(apiService, 'get', async () => task('Completed'))
  assert.equal(await getRunningPlatformTask('steam'), null)
  mock.method(apiService, 'get', async () => ({ success: true, task: null }))
  assert.equal(await getRunningPlatformTask('steam'), null)
})
