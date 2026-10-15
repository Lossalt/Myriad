import type { Page } from '@playwright/test'
import { expect, test } from '@playwright/test'

async function openEditor(page: Page, shape: string, size = 180_000) {
  let revision = 1
  await page.route('**/api/**', async route => {
    if (route.request().url().includes('csrf-token')) { await route.fulfill({ json: { csrf_token: null } }); return }
    const body = route.request().postDataJSON() ?? {}
    const id = Number(/\/docs\/(\d+)/.exec(route.request().url())?.[1] ?? 1)
    await route.fulfill({ json: { success: true, id: 100 + id, link: '/studio/notes/test', doc: {
      id, item_id: null, title: body.title ?? 'Large note', content_md: body.content_md ?? '', topic: null, image: null,
      published_at: null, status: 'draft', scheduled_at: null, revision: ++revision, updated_at: 0, last_error: null,
    } } })
  })
  await page.goto(`/noteLargeDocument.html?shape=${shape}&size=${size}`)
  await expect(page.locator('#editor')).toBeVisible()
  await page.evaluate(() => {
    const root = document.getElementById('editor')!
    const target = root.querySelector('td:nth-child(2)') ?? root.querySelector('code') ?? root.querySelector('p')!
    root.focus()
    const range = document.createRange()
    range.selectNodeContents(target)
    range.collapse(false)
    getSelection()!.removeAllRanges()
    getSelection()!.addRange(range)
  })
}

for (const shape of ['paragraph', 'pre', 'table']) {
  test(`large ${shape}: real typing, current save/publish, undo, and cached conversion`, async ({ page }) => {
    test.setTimeout(60_000)
    await openEditor(page, shape)
    for (let i = 0; i < 16; i++) await page.keyboard.insertText(String(i % 10))
    const typed = await page.evaluate(() => {
      const state = (window as any).noteLarge.snapshot()
      return { count: state.samples.length, samples: state.samples, commits: state.commitSamples, paints: state.paintSamples, errors: state.errors, unicodeCountCorrect: state.chars === [...state.md].length,
        selection: getSelection()!.anchorNode?.textContent?.includes('0123456789012345'), copiedToDom: document.getElementById('editor')!.hasAttribute('data-note-visual') }
    })
    expect(typed.count).toBe(16)
    expect(typed.selection).toBe(true)
    expect(typed.copiedToDom).toBe(false)
    expect(typed.errors).toEqual([])
    expect(typed.unicodeCountCorrect).toBe(true)
    const comparison = await page.evaluate(() => (window as any).noteLarge.compare()) as { legacy: number[]; current: number[] }
    const total = (numbers: number[]) => numbers.reduce((sum, n) => sum + n, 0)
    // Relative in-page comparison avoids a device-specific absolute FPS gate.
    expect(total(comparison.current.slice(1))).toBeLessThan(total(comparison.legacy.slice(1)))
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await expect.poll(() => page.evaluate(() => { const state = (window as any).noteLarge.snapshot(); return state.saved === state.md })).toBe(true)
    await page.getByRole('button', { name: 'Publish', exact: true }).click()
    await expect.poll(() => page.evaluate(() => (window as any).noteLarge.snapshot().published)).toBe(true)
    await page.evaluate(() => { (window as any).expectedLargeText = (window as any).noteLarge.snapshot().md })
    await page.getByRole('button', { name: 'Undo', exact: true }).click()
    await expect.poll(() => page.evaluate(() => (window as any).noteLarge.snapshot().md !== (window as any).expectedLargeText)).toBe(true)
    await page.getByRole('button', { name: 'Redo', exact: true }).click()
    await expect.poll(() => page.evaluate(() => (window as any).noteLarge.snapshot().md === (window as any).expectedLargeText)).toBe(true)
    console.log(JSON.stringify({ shape, typed, comparison }))
    await test.info().attach(`large-${shape}-timings`, { body: JSON.stringify({ typed, comparison }), contentType: 'application/json' })
  })
}

test('IME and document switching preserve the latest large draft before recovery debounce', async ({ page }) => {
  await openEditor(page, 'paragraph', 80_000)
  await page.evaluate(() => document.getElementById('editor')!.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true })))
  await page.keyboard.insertText('入力中😀')
  await page.waitForTimeout(400)
  expect(await page.evaluate(() => (window as any).noteLarge.snapshot().writes)).toBe(0)
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide')))
  expect(await page.evaluate(() => { const state = (window as any).noteLarge.snapshot(); return state.recovery?.fields.contentMd === state.md })).toBe(true)
  await page.keyboard.insertText('確定')
  await page.evaluate(() => document.getElementById('editor')!.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '入力中😀確定' })))
  await expect.poll(() => page.evaluate(() => { const state = (window as any).noteLarge.snapshot(); return state.saved === state.md && state.md.endsWith('入力中😀確定') })).toBe(true)
  await page.keyboard.insertText('last unsaved character')
  await page.evaluate(() => { (window as any).expectedLargeText = (window as any).noteLarge.snapshot().md })
  await page.getByRole('button', { name: 'Switch document', exact: true }).click()
  await expect(page.locator('#editor')).toHaveText('Second document')
  expect(await page.evaluate(() => (window as any).noteLarge.snapshot().recovery?.fields.contentMd === (window as any).expectedLargeText)).toBe(true)
})

test('a one-million-character single block keeps ordinary native input on the direct text path', async ({ page }) => {
  test.setTimeout(60_000)
  await openEditor(page, 'paragraph', 1_000_000)
  // Keep the stress draft unacknowledged while measuring expensive native layout.
  await page.evaluate(() => document.getElementById('editor')!.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true })))
  await page.keyboard.insertText('終😀')
  await page.waitForTimeout(100)
  const result = await page.evaluate(() => { const state = (window as any).noteLarge.snapshot(); return { current: state.md.endsWith('終😀'), timings: state.samples, commits: state.commitSamples, paints: state.paintSamples } })
  expect(result.current).toBe(true)
  await page.getByRole('button', { name: 'Publish', exact: true }).click()
  expect(await page.evaluate(() => (window as any).noteLarge.snapshot().published)).toBe(false)
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide')))
  expect(await page.evaluate(() => { const state = (window as any).noteLarge.snapshot(); return state.recovery?.fields.contentMd === state.md && state.md.length > 1_000_000 })).toBe(true)
  console.log(JSON.stringify({ shape: 'million-paragraph', result }))
  await test.info().attach('million-character-input', { body: JSON.stringify(result), contentType: 'application/json' })
})
