import type { AnnotationItem } from '../../services/phantasiaiApi'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { describe, it } from 'node:test'
import {
  connectIsland,
  findIslandHost,
  paintAnnotations,
  termCandidates,
  unpaintAnnotations,
} from './companionBridge.ts'

const require = createRequire(import.meta.url)
const { JSDOM } = require(
  require.resolve('jsdom', {
    paths: [require.resolve('isomorphic-dompurify')],
  }),
)

function page(body: string) {
  const dom = new JSDOM(
    `<article><div class="content">${body}<details class="myriad-island"><summary class="myriad-island__pill"><span class="myriad-island__label">AI 伴读</span></summary><iframe class="myriad-island__panel" src="/embed/companion/7"></iframe></details></div></article>`,
    { url: 'https://myriad.test/blog/posts/a/' },
  )
  const doc = dom.window.document as Document
  const frame = doc.querySelector('iframe')!
  return { dom, doc, frame, root: doc.querySelector('.content') as HTMLElement }
}

function note(term: string, extra: Partial<AnnotationItem> = {}): AnnotationItem {
  return {
  type: 'term',
  term,
  explanation: `${term} 的解释`,
  ...extra,
}
}

describe('companion bridge', () => {
  it('finds the article the island was written into, from the frame element', () => {
    const { frame, root } = page('<p>正文</p>')
    const host = findIslandHost(frame)
    assert.equal(host?.root, root)
    assert.equal(host?.island.tagName, 'DETAILS')
    assert.equal(findIslandHost(null), null)
  })

  it('prefers the AI offset, then every occurrence in order', () => {
    assert.deepEqual(termCandidates('a Rust b Rust', 'Rust', 9), [9, 2])
    assert.deepEqual(termCandidates('a Rust b Rust', 'Rust', 1), [2, 9])
    assert.deepEqual(termCandidates('abc', ''), [])
  })

  it('marks plain text once per term, skipping links, code and the island', () => {
    const { root } = page(
      '<p><a href="/x">Rust</a> 与 <code>Rust</code></p><pre>Rust</pre><p>后来 Rust 稳定了，Rust 很快。</p>',
    )
    const painted = paintAnnotations(root, [note('不存在'), note('Rust'), note('伴读')])
    assert.deepEqual(painted, [1])
    const marks = [...root.querySelectorAll('mark.myriad-annot')]
    assert.equal(marks.length, 1)
    assert.equal(marks[0].textContent, 'Rust')
    assert.equal(marks[0].closest('p')?.textContent, '后来 Rust 稳定了，Rust 很快。')
    assert.equal(root.querySelector('a mark, pre mark, code mark, details mark'), null)
  })

  it('does not nest a short term inside a longer one, and unpaints cleanly', () => {
    const { root } = page('<p>Rust 编译器 很严格。</p>')
    const before = root.innerHTML
    paintAnnotations(root, [note('Rust'), note('Rust 编译器')])
    const marks = [...root.querySelectorAll('mark.myriad-annot')]
    assert.deepEqual(marks.map((mark) => mark.textContent), ['Rust 编译器'])
    assert.equal(root.querySelector('mark mark'), null)
    unpaintAnnotations(root)
    assert.equal(root.innerHTML, before)
  })

  it('shows AI text as text, opens the island on pick, and leaves nothing behind', () => {
    const { dom, doc, frame, root } = page('<p>谈谈 XSS 吧。</p>')
    const host = findIslandHost(frame)!
    const picked: number[] = []
    const link = connectIsland(host, {
      typeLabel: (type) => `类型:${type}`,
      onPick: (index) => picked.push(index),
    })
    const hostile = note('XSS', { explanation: '<img src=x onerror=alert(1)>' })
    assert.deepEqual(link.paint([hostile]), [0])
    const mark = root.querySelector('mark.myriad-annot') as HTMLElement
    mark.dispatchEvent(new dom.window.MouseEvent('mouseover', { bubbles: true }))
    const tip = doc.getElementById('myriad-annot-tip')!
    assert.equal(tip.hidden, false)
    assert.equal(tip.querySelector('img'), null)
    assert.match(tip.textContent ?? '', /<img src=x/)
    assert.equal(mark.getAttribute('aria-describedby'), 'myriad-annot-tip')

    mark.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }))
    assert.deepEqual(picked, [0])
    assert.equal(host.island.open, true)
    doc.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Escape' }))
    assert.equal(host.island.open, false)

    link.setLabel('Reading companion')
    link.setPlaying('正在播放 2/9')
    assert.equal(host.island.dataset.state, 'playing')
    assert.equal(host.island.querySelector('.myriad-island__label')?.textContent, '正在播放 2/9')
    link.setPlaying(null)
    assert.equal(host.island.dataset.state, undefined)
    assert.equal(host.island.querySelector('.myriad-island__label')?.textContent, 'Reading companion')

    link.dispose()
    assert.equal(doc.getElementById('myriad-annot-tip'), null)
    assert.equal(root.querySelector('mark'), null)
    assert.equal(host.island.querySelector('.myriad-island__label')?.textContent, 'AI 伴读')
  })
})
