import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  emptyNoteWidgetText,
  hasNoteWidgetMarkup,
  noteWidgetTypesInHtml,
  stampNoteWidgetNotProse,
} from './noteWidgetHtml.ts'
import { noteWidgetInstanceId } from './noteWidgetId.ts'

const dir = dirname(fileURLToPath(import.meta.url))
const mount = readFileSync(join(dir, 'noteWidgetMount.tsx'), 'utf8')
const editor = [
  'NoteEditor.tsx',
  'NoteEditorView.tsx',
  'useNoteEditorSession.ts',
  'useNoteEditorSidecar.ts',
  'useNoteEditorPreview.ts',
  'useNoteEditorFormat.ts',
]
  .map((file) => readFileSync(join(dir, file), 'utf8'))
  .join('\n')

describe('hasNoteWidgetMarkup', () => {
  it('认发布 HTML 里的占位，不靠一段纯文本', () => {
    assert.equal(
      hasNoteWidgetMarkup(
        '<div class="note-widget" data-widget="weather" data-size="2x2">weather</div>',
      ),
      true,
    )
    assert.equal(hasNoteWidgetMarkup('<div data-widget="quote">quote</div>'), true)
    assert.equal(hasNoteWidgetMarkup('<p>note-widget</p>'), false)
    assert.equal(hasNoteWidgetMarkup(':::widget weather 2x2'), false)
    assert.equal(hasNoteWidgetMarkup(''), false)
    assert.equal(hasNoteWidgetMarkup(null), false)
  })

  it('预览和阅读器都拉真数据，不走宫格样本', () => {
    assert.match(mount, /isPreview=\{false\}/)
    assert.doesNotMatch(mount, /isPreview(?!\s*=\s*\{false\})/)
  })

  it('水合时给旧占位补 not-prose，挡住阅读器 typography', () => {
    assert.match(mount, /host\.classList\.add\('not-prose'\)/)
  })

  it('只认占位上的实例配置，不搬首页宫格', () => {
    assert.doesNotMatch(mount, /useHomeWidgetDefaults|homeWidgetConfigsFromUi|widgetPreviewConfig/)
    assert.match(mount, /\.\.\.widgetHostConfig\(widgetType\.id\),[\t\v\f\r \xA0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000\uFEFF]*\n\s*\.\.\.instanceConfig/)
  })

  it('从这棵树上 portal 进去，不另开 createRoot，也不造 MemoryRouter', () => {
    assert.match(mount, /createPortal\(/)
    assert.doesNotMatch(mount, /createRoot|MemoryRouter/)
    assert.doesNotMatch(mount, /getBuiltinWidgets|useTappWidgets/)
    assert.match(editor, /visualWidgets\.portals/)
    assert.match(editor, /previewWidgets\.portals/)
    assert.doesNotMatch(editor, /mountNoteWidgets/)
    assert.match(editor, /visualWidgets\.refresh\(\)/)
    assert.match(editor, /visualMarkdownStamp\.get\(el\) === contentMd/)
    assert.match(editor, /pane !== 'preview'/)
    assert.match(editor, /preloadNoteWidgets\(/)
    assert.match(editor, /hidden=\{!html\}/)
    assert.doesNotMatch(editor, /: html \? \(/)
  })

  it('同一类型两张各有自己的实例 id，不共用 preview- 前缀', () => {
    const a = {} as HTMLElement
    const b = {} as HTMLElement
    const first = noteWidgetInstanceId(a, 'weather')
    const second = noteWidgetInstanceId(b, 'weather')
    assert.match(first, /^note-weather-\d+$/)
    assert.notEqual(first, second)
    assert.equal(noteWidgetInstanceId(a, 'weather'), first)
    assert.doesNotMatch(mount, /preview-\$\{/)
  })
})

describe('noteWidgetTypesInHtml', () => {
  it('按出现顺序列出类型，重复的只留一次', () => {
    assert.deepEqual(
      noteWidgetTypesInHtml(
        '<div class="note-widget" data-widget="friend-links"></div><div data-widget="Friend-Links"></div><div data-widget="report-bilibili"></div>',
      ),
      ['friend-links', 'report-bilibili'],
    )
    assert.deepEqual(noteWidgetTypesInHtml('<p>nope</p>'), [])
    assert.deepEqual(noteWidgetTypesInHtml(''), [])
  })
})

describe('emptyNoteWidgetText', () => {
  it('只清掉纯文本类型名，有子节点的占位不动', () => {
    assert.equal(
      emptyNoteWidgetText(
        '<div class="note-widget" data-widget="game-presence" data-size="4x2">game-presence</div>',
      ),
      '<div class="note-widget" data-widget="game-presence" data-size="4x2"></div>',
    )
    assert.equal(
      emptyNoteWidgetText(
        '<div class="note-widget" data-widget="weather"><div class="note-widget__face">x</div></div>',
      ),
      '<div class="note-widget" data-widget="weather"><div class="note-widget__face">x</div></div>',
    )
  })
})

describe('stampNoteWidgetNotProse', () => {
  it('旧占位补上 not-prose，已经有的不重复', () => {
    assert.equal(
      stampNoteWidgetNotProse(
        '<div class="note-widget" data-widget="game-presence" data-size="4x2">game-presence</div>',
      ),
      '<div class="note-widget not-prose" data-widget="game-presence" data-size="4x2">game-presence</div>',
    )
    assert.equal(
      stampNoteWidgetNotProse(
        '<div class="note-widget not-prose" data-widget="weather" data-size="2x2">weather</div>',
      ),
      '<div class="note-widget not-prose" data-widget="weather" data-size="2x2">weather</div>',
    )
    assert.equal(
      stampNoteWidgetNotProse('<div class="note-columns"><div class="note-column">左</div></div>'),
      '<div class="note-columns"><div class="note-column">左</div></div>',
    )
  })
})
