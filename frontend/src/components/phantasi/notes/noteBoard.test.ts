import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { noteEditorStatus, noteScheduleLabel } from './noteBoard.ts'

describe('noteScheduleLabel', () => {
  it('定时时间写成看得见的钟点', () => {
    const label = noteScheduleLabel(Date.UTC(2026, 2, 3, 8, 30), 'en-US')
    assert.match(label, /Mar/)
    assert.match(label, /3/)
  })

  it('缺时间是空串', () => {
    assert.equal(noteScheduleLabel(null, 'en-US'), '')
    assert.equal(noteScheduleLabel(Number.NaN, 'en-US'), '')
  })
})

describe('noteEditorStatus', () => {
  it('草稿保存后把状态和时间说清楚', () => {
    assert.equal(
      noteEditorStatus(
        { status: 'draft', savedHint: true },
        {
          failed: '失败',
          scheduled: '定时',
          published: '已发布',
          draft: '草稿',
          saved: '已保存',
        },
        'en-US',
      ),
      '草稿 · 已保存',
    )
  })
})
