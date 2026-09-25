import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  AGENT_ATTACH_MAX_COUNT,
  attachErrorFor,
  attachmentsForDisplay,
  attachmentsForRequest,
  collectAttachments,
  isAttachableFile,
} from './agentAttachments'

function file(name: string, type: string, size: number): File {
  return new File([new Uint8Array(size)], name, { type })
}

describe('agentAttachments', () => {
  it('accepts images and common text files', () => {
    assert.equal(isAttachableFile(file('a.png', 'image/png', 10)), true)
    assert.equal(isAttachableFile(file('n.md', 'text/markdown', 10)), true)
    assert.equal(isAttachableFile(file('n.txt', 'text/plain', 10)), true)
    assert.equal(isAttachableFile(file('notes.md', '', 10)), true)
    assert.equal(
      isAttachableFile(file('x.exe', 'application/x-msdownload', 10)),
      false,
    )
  })

  it('rejects over the count and size caps', () => {
    assert.equal(
      attachErrorFor(file('a.png', 'image/png', 10), AGENT_ATTACH_MAX_COUNT),
      'tooMany',
    )
    assert.equal(
      attachErrorFor(file('a.png', 'image/png', 9 * 1024 * 1024), 0),
      'tooLarge',
    )
    assert.equal(
      attachErrorFor(file('a.bin', 'application/octet-stream', 10), 0),
      'unsupported',
    )
  })

  it('sends the model-size image and keeps it out of the chat', () => {
    const image = {
      id: '1',
      name: 'a.png',
      mime: 'image/png',
      size: 12,
      previewUrl: 'data:image/webp;base64,small',
      modelImage: 'data:image/jpeg;base64,big',
    }
    assert.deepEqual(attachmentsForRequest([image]), [
      { name: 'a.png', mime: 'image/png', size: 12, image: 'data:image/jpeg;base64,big' },
    ])
    const [kept] = attachmentsForDisplay([image])
    assert.equal(kept.modelImage, undefined)
    assert.equal(kept.previewUrl, 'data:image/webp;base64,small')
  })

  it('strips preview urls before sending to the backend', () => {
    const sent = attachmentsForRequest([
      {
        id: '1',
        name: 'a.png',
        mime: 'image/png',
        size: 12,
        previewUrl: 'data:image/png;base64,xx',
      },
      {
        id: '2',
        name: 'n.txt',
        mime: 'text/plain',
        size: 4,
        text: 'hi',
      },
    ])
    assert.deepEqual(sent, [
      { name: 'a.png', mime: 'image/png', size: 12 },
      { name: 'n.txt', mime: 'text/plain', size: 4, text: 'hi' },
    ])
  })

  it('collects text files until the cap and reports the first error', async () => {
    const files = Array.from({ length: 5 }, (_, i) =>
      file(`n${i}.txt`, 'text/plain', 4),
    )
    const result = await collectAttachments(files, [])
    assert.equal(result.attachments.length, AGENT_ATTACH_MAX_COUNT)
    assert.equal(result.error, 'tooMany')
    assert.equal(result.attachments[0].name, 'n0.txt')
    assert.ok(result.attachments[0].text !== undefined)
  })
})

it('images decode at bounded sizes and release their bitmaps', async () => {
  let closed = 0
  const previousBitmap = globalThis.createImageBitmap
  const previousDocument = globalThis.document
  globalThis.createImageBitmap = (async (_file: unknown, options: ImageBitmapOptions) => {
    const edge = options.resizeWidth ?? Infinity
    assert.ok(edge <= 1024)
    return { width: edge, height: edge / 2, close: () => { closed += 1 } }
  }) as typeof createImageBitmap
  globalThis.document = { createElement: () => ({ width: 0, height: 0, getContext: () => ({ drawImage: () => {}, fillRect: () => {} }), toDataURL: (type: string) => `data:${type};base64,${type === 'image/webp' ? 'small' : 'big'}` }) } as unknown as Document
  try {
    const result = await collectAttachments([file('large.png', 'image/png', 1024)], [])
    assert.equal(result.attachments[0].previewUrl, 'data:image/webp;base64,small')
    assert.equal(result.attachments[0].modelImage, 'data:image/jpeg;base64,big')
    assert.equal(closed, 2)
  } finally {
    globalThis.createImageBitmap = previousBitmap
    globalThis.document = previousDocument
  }
})
