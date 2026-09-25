import type { ProgressEvent } from '../../services/agent'
import assert from 'node:assert/strict'
import { afterEach, mock, test } from 'node:test'
import { DONE_LINGER_MS, ERROR_LINGER_MS } from './agentStatus'
import {
  getAgentLaneLoading,
  getAgentStatusSnapshot,
  pushAgentStatusEvent,
  resetAgentStatus,
  setAgentLaneLoading,
  setAgentStatusAwaitingConfirmation,
  setAgentStatusRecording,
  setAgentStatusThinking,
  subscribeAgentStatus,
} from './agentStatusStore'

const runStarted: ProgressEvent = { type: 'run_started', runId: 'run_1' }

const stepStarted: ProgressEvent = {
  type: 'step_started',
  stepId: 'step_1',
  stepIndex: 0,
  totalSteps: 2,
  capabilityName: 'search',
  description: '正在查天气',
}

afterEach(() => {
  mock.timers.reset()
  setAgentStatusRecording(false)
  setAgentLaneLoading('work', false)
  setAgentLaneLoading('chat', false)
  resetAgentStatus()
})

test('订阅者只在状态真的变了的时候被叫醒', () => {
  let notifications = 0
  const unsubscribe = subscribeAgentStatus(() => {
    notifications += 1
  })

  pushAgentStatusEvent(runStarted)
  assert.equal(getAgentStatusSnapshot().status, 'thinking')
  assert.equal(notifications, 1)

  pushAgentStatusEvent({ type: 'session_created', sessionId: 's1' })
  assert.equal(notifications, 1)

  pushAgentStatusEvent(stepStarted)
  assert.equal(notifications, 2)

  unsubscribe()
  pushAgentStatusEvent({ type: 'task_completed' } as ProgressEvent)
  assert.equal(notifications, 2)
})

test('状态没变时快照引用保持不变', () => {
  pushAgentStatusEvent(runStarted)
  const before = getAgentStatusSnapshot()
  pushAgentStatusEvent({
    type: 'session_title_updated',
    sessionId: 's1',
    title: '查天气',
  })
  assert.equal(getAgentStatusSnapshot(), before)
})

test('发出去立刻占住思考，已经在跑就不要打回去', () => {
  setAgentStatusThinking()
  assert.equal(getAgentStatusSnapshot().status, 'thinking')

  pushAgentStatusEvent(stepStarted)
  assert.equal(getAgentStatusSnapshot().status, 'working')

  setAgentStatusThinking()
  assert.equal(getAgentStatusSnapshot().status, 'working')
})

test('录音叠在事件状态之上，但不抢正在跑的任务', () => {
  setAgentStatusRecording(true)
  assert.equal(getAgentStatusSnapshot().status, 'listening')

  pushAgentStatusEvent(runStarted)
  pushAgentStatusEvent(stepStarted)
  assert.equal(getAgentStatusSnapshot().status, 'working')

  setAgentStatusRecording(false)
  assert.equal(getAgentStatusSnapshot().status, 'working')
})

test('完成与出错各自停留一会儿，然后自己退回空闲', () => {
  mock.timers.enable({ apis: ['setTimeout'] })

  pushAgentStatusEvent(runStarted)
  pushAgentStatusEvent({
    type: 'task_completed',
    taskId: 't1',
    success: true,
    response: { success: true, message: '', responseType: 'answer' },
  } as ProgressEvent)
  assert.equal(getAgentStatusSnapshot().status, 'done')

  mock.timers.tick(DONE_LINGER_MS - 1)
  assert.equal(getAgentStatusSnapshot().status, 'done')
  mock.timers.tick(1)
  assert.equal(getAgentStatusSnapshot().status, 'idle')

  pushAgentStatusEvent({ type: 'error', message: '网络断了', code: 'NETWORK' })
  assert.equal(getAgentStatusSnapshot().status, 'error')
  mock.timers.tick(DONE_LINGER_MS)
  assert.equal(getAgentStatusSnapshot().status, 'error')
  mock.timers.tick(ERROR_LINGER_MS - DONE_LINGER_MS)
  assert.equal(getAgentStatusSnapshot().status, 'idle')
})

test('中断立刻回空闲，不等停留时间，也不会被之前的定时器再改一次', () => {
  mock.timers.enable({ apis: ['setTimeout'] })

  pushAgentStatusEvent(runStarted)
  pushAgentStatusEvent({ type: 'error', message: '出错', code: 'X' })
  resetAgentStatus()
  assert.equal(getAgentStatusSnapshot().status, 'idle')

  pushAgentStatusEvent(runStarted)
  mock.timers.tick(ERROR_LINGER_MS * 2)
  assert.equal(getAgentStatusSnapshot().status, 'thinking')
})

test('敏感确认落到等回话那一档，带上问题本身', () => {
  pushAgentStatusEvent(runStarted)
  setAgentStatusAwaitingConfirmation('确认删除这 3 个文件？')
  assert.deepEqual(getAgentStatusSnapshot(), {
    status: 'needsInput',
    detail: '确认删除这 3 个文件？',
  })
})

test('车道占用变了就算岛状态没变也要叫醒订阅者', () => {
  setAgentStatusThinking()
  let notifications = 0
  const unsubscribe = subscribeAgentStatus(() => {
    notifications += 1
  })
  setAgentLaneLoading('chat', true)
  assert.equal(getAgentLaneLoading('chat'), true)
  assert.equal(getAgentLaneLoading('work'), false)
  assert.equal(notifications, 1)
  setAgentLaneLoading('chat', true)
  assert.equal(notifications, 1)
  setAgentLaneLoading('chat', false)
  assert.equal(getAgentLaneLoading('chat'), false)
  assert.equal(notifications, 2)
  unsubscribe()
})
