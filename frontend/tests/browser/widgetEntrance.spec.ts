import { expect, test } from '@playwright/test'

declare global {
  interface Window {
    widgetEntranceFixture: {
      mountAdmissionProbe: () => void
      mountLifetimeProbe: () => void
      batchRead: (callback: () => void) => void
      batchWrite: (callback: () => void) => void
      scheduleTask: (callback: () => void) => void
      startPage: (id: string) => void
      runPageCleanup: (id: string) => void
      mountEntranceModes: () => Promise<void>
      mountWelcomeEntrance: (
        size: '2x2' | '4x2',
        immediate?: boolean,
      ) => Promise<void>
      mountWidgetEntrance: () => Promise<void>
      mountHeldEntrance: () => Promise<void>
      coordinator: typeof import('../../src/hooks/animation/coordinator').coordinator
    }
  }
}

test.beforeEach(async ({ page }) => {
  await page.route('**/api/**', (route) =>
    route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: {} }),
    }),
  )
  await page.goto('/widgetEntrance.html')
  await page.waitForFunction(() => Boolean(window.widgetEntranceFixture))
})

test('widget entrance uses one native timeline for transform and opacity', async ({
  page,
}) => {
  await page.evaluate(() => window.widgetEntranceFixture.mountWidgetEntrance())
  const card = page.locator('[data-entrance-probe="1"]')
  await expect(card).toBeAttached()
  const result = await card.evaluate(async (element) => {
    let animations: Animation[] = []
    for (let i = 0; i < 40; i++) {
      animations = element
        .getAnimations()
        .filter((animation) => animation.playState === 'running')
      if (
        animations.some((animation) =>
          (animation.effect as KeyframeEffect)
            .getKeyframes()
            .some((frame) => 'transform' in frame),
        )
      ) {
        break
}
      await new Promise(requestAnimationFrame)
    }
    // Capture promises before the owner cancels finished animations during cleanup.
    const completed = animations.map(animation => animation.finished)
    const native = animations.map((animation) => ({
      keys: Object.keys((animation.effect as KeyframeEffect).getKeyframes()[0]),
      duration: animation.effect!.getTiming().duration,
      easing: animation.effect!.getTiming().easing,
    }))
    const frames: { scale: number; y: number; opacity: number }[] = []
    for (let i = 0; i < 50; i++) {
      await new Promise(requestAnimationFrame)
      const css = getComputedStyle(element)
      const transform = new DOMMatrixReadOnly(css.transform)
      frames.push({
        scale: transform.a,
        y: transform.f,
        opacity: Number(css.opacity),
      })
      if (animations.every(animation => animation.playState === 'finished')) break
    }
    // Near-one rounded CSS values do not prove the native animation has finished.
    await Promise.all(completed)
    const finalCss = getComputedStyle(element)
    const finalTransform = new DOMMatrixReadOnly(finalCss.transform)
    frames.push({ scale: finalTransform.a, y: finalTransform.f, opacity: Number(finalCss.opacity) })
    return { native, frames }
  })
  expect(
    result.native.some((animation) => animation.keys.includes('transform')),
  ).toBe(true)
  expect(
    result.native.some((animation) => animation.keys.includes('opacity')),
  ).toBe(true)
  expect(new Set(result.native.map((animation) => animation.duration))).toEqual(
    new Set([560]),
  )
  expect(new Set(result.native.map((animation) => animation.easing)).size).toBe(
    1,
  )
  expect(result.frames.length).toBeGreaterThan(5)
  for (let i = 1; i < result.frames.length; i++) {
    expect(result.frames[i].scale).toBeGreaterThanOrEqual(
      result.frames[i - 1].scale - 0.00001,
    )
    expect(result.frames[i].y).toBeLessThanOrEqual(
      result.frames[i - 1].y + 0.00001,
    )
    expect((result.frames[i].scale - 0.9) / 0.1).toBeCloseTo(
      result.frames[i].opacity,
      3,
    )
  }
  expect(result.frames.at(-1)).toEqual({ scale: 1, y: 0, opacity: 1 })
})

test('widget contents ride the parent entrance but retain later content transitions', async ({
  page,
}) => {
  await page.evaluate(() => window.widgetEntranceFixture.mountWidgetEntrance())
  const card = page.locator('[data-entrance-probe="2"]')
  await expect(card).toBeAttached()
  const frames = await card.evaluate(async (element) => {
    const frames: {
      parentOpacity: number
      childOpacity: number
      childX: number
    }[] = []
    while (element.getAttribute('data-complete') !== 'true') {
      const child = element.querySelector('[data-entrance-content]')!
      const css = getComputedStyle(child)
      frames.push({
        parentOpacity: Number(getComputedStyle(element).opacity),
        childOpacity: Number(css.opacity),
        childX: new DOMMatrixReadOnly(css.transform).e,
      })
      if (frames.length > 150) break
      await new Promise(requestAnimationFrame)
    }
    return frames
  })
  expect(
    frames.some((frame) => frame.parentOpacity > 0 && frame.parentOpacity < 1),
  ).toBe(true)
  expect(
    frames.every((frame) => frame.childOpacity === 1 && frame.childX === 0),
  ).toBe(true)
  await expect(card).toHaveAttribute('data-complete', 'true')
  await card.getByRole('button', { name: 'Change content' }).click()
  const next = card.locator('[data-entrance-content="1"]')
  await expect(next).toHaveCSS('opacity', '0')
  await expect(next).toHaveCSS('opacity', '1')
})

for (const size of ['2x2', '4x2'] as const) {
  test(`real welcome ${size} retains inner entrance and waits for its card`, async ({
    page,
  }) => {
    await page.evaluate(
      (size) => window.widgetEntranceFixture.mountWelcomeEntrance(size),
      size,
    )
    const heading = page.locator('.widget-grid-item h2')
    await expect(heading).toBeAttached()
    const greeting = heading.locator('..')
    await expect(greeting).toHaveCSS('opacity', '0')
    // Even after the inner animation's normal duration, it must still await the card.
    await page.waitForTimeout(650)
    await expect(greeting).toHaveCSS('opacity', '0')
    await page.evaluate(() =>
      window.widgetEntranceFixture.coordinator.completePageTransition(
        'welcome-audit',
      ),
    )
    const samples = await greeting.evaluate(async (element) => {
      const samples: { inner: number; outer: number }[] = []
      for (let frame = 0; frame < 120; frame++) {
        const inner = Number(getComputedStyle(element).opacity)
        const outer = Number(
          getComputedStyle(element.closest('.widget-grid-item')!).opacity,
        )
        samples.push({ inner, outer })
        if (inner === 1 && outer === 1) break
        await new Promise(requestAnimationFrame)
      }
      return samples
    })
    expect(
      samples.some(
        (sample) => sample.inner > 0 && sample.inner < 1 && sample.outer > 0,
      ),
    ).toBe(true)
    expect(samples.at(-1)).toEqual({ inner: 1, outer: 1 })
  })
}

test('first lazy welcome animates its greeting under the home route presence boundary', async ({
  page,
}) => {
  await page.evaluate(() =>
    window.widgetEntranceFixture.mountWelcomeEntrance('4x2', true),
  )
  const heading = page.locator('.widget-grid-item h2')
  await expect(heading).toBeAttached()
  const samples = await heading.locator('..').evaluate(async (element) => {
    const samples: number[] = []
    for (let frame = 0; frame < 70; frame++) {
      samples.push(Number(getComputedStyle(element).opacity))
      await new Promise(requestAnimationFrame)
    }
    return samples
  })
  expect(samples.some((opacity) => opacity > 0 && opacity < 0.99)).toBe(true)
  expect(samples.at(-1)).toBe(1)
})

test('a card holds its entrance until its content commits, then enters with it', async ({ page }) => {
  await page.evaluate(() => window.widgetEntranceFixture.mountHeldEntrance())
  const card = page.locator('.widget-grid-item')
  await expect(card).toBeAttached()
  const pending = await card.evaluate(async (element) => {
    const samples: number[] = []
    for (let frame = 0; frame < 30; frame++) {
      samples.push(Number(getComputedStyle(element).opacity))
      await new Promise(requestAnimationFrame)
    }
    return samples
  })
  expect(pending.every(opacity => opacity === 0)).toBe(true)
  const entering = await card.evaluate(async (element) => {
    ;(window as unknown as { releaseHeldEntrance: () => void }).releaseHeldEntrance()
    const samples: { opacity: number, content: boolean }[] = []
    for (let frame = 0; frame < 90; frame++) {
      samples.push({
        opacity: Number(getComputedStyle(element).opacity),
        content: Boolean(element.querySelector('[data-held-content]')),
      })
      await new Promise(requestAnimationFrame)
    }
    return samples
  })
  // Never an empty card on screen: every visible frame already shows the content.
  expect(entering.filter(sample => sample.opacity > 0).every(sample => sample.content)).toBe(true)
  expect(entering.some(sample => sample.opacity > 0 && sample.opacity < 1)).toBe(true)
  expect(entering.at(-1)).toEqual({ opacity: 1, content: true })
})

test('enabling motion after a disabled entrance never hides or replays shown contents', async ({
  page,
}) => {
  await page.evaluate(() => window.widgetEntranceFixture.mountEntranceModes())
  const card = page.locator('[data-mode-probe]')
  const content = page.locator('[data-mode-content]')
  await expect(card).toHaveAttribute('data-phase', 'disabled')
  await expect(content).toHaveCSS('opacity', '1')
  await page.getByRole('button', { name: 'Toggle motion' }).click()
  await expect(card).toHaveAttribute('data-phase', 'complete')
  const samples = await content.evaluate(async (element) => {
    const samples: string[] = []
    for (let frame = 0; frame < 30; frame++) {
      samples.push(getComputedStyle(element).opacity)
      await new Promise(requestAnimationFrame)
    }
    return samples
  })
  expect(samples.every((opacity) => opacity === '1')).toBe(true)
})

test('reduced motion leaves the lazy welcome visible without waiting on an animation', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.evaluate(() =>
    window.widgetEntranceFixture.mountWelcomeEntrance('4x2', true),
  )
  const greeting = page.locator('.widget-grid-item h2').locator('..')
  await expect(greeting).toHaveCSS('opacity', '1')
  const state = await greeting.evaluate((element) => ({
    opacity: getComputedStyle(element.closest('.widget-grid-item')!).opacity,
    animations: element.getAnimations().length,
  }))
  expect(state).toEqual({ opacity: '1', animations: 0 })
})

test('TAPP and home entrances share the page gate and release the same concurrency pool', async ({ page }) => {
  await page.evaluate(() => window.widgetEntranceFixture.mountAdmissionProbe())
  await expect(page.locator('[data-admission="tapp"]')).toHaveText('false')
  await expect(page.locator('[data-admission="widget"]')).toHaveText('false')
  await page.evaluate(() => window.widgetEntranceFixture.coordinator.completePageTransition('admission-audit'))
  await expect(page.locator('[data-admission="tapp"]')).toHaveText('true')
  await expect(page.locator('[data-admission="widget"]')).toHaveText('false')
  await page.getByRole('button', { name: 'Complete TAPP', exact: true }).click()
  await expect(page.locator('[data-admission="widget"]')).toHaveText('true')
  await page.getByRole('button', { name: 'Complete widget', exact: true }).click()
  expect(await page.evaluate(() => window.widgetEntranceFixture.coordinator.getConcurrencyStatus().activeSlots)).toBe(0)
})

test('reordering an admitted card never hides it or admits it twice', async ({ page }) => {
  await page.evaluate(() => {
    const fixture = window.widgetEntranceFixture
    fixture.mountAdmissionProbe()
    fixture.coordinator.completePageTransition('admission-audit')
  })
  await expect(page.locator('[data-admission="tapp"]')).toHaveText('true')
  const before = await page.evaluate(() => window.widgetEntranceFixture.coordinator.getConcurrencyStatus().totalAcquired)
  await page.getByText('Reorder', { exact: true }).click()
  await expect(page.locator('[data-admission="tapp"]')).toHaveText('true')
  await expect(page.locator('[data-admission="widget"]')).toHaveText('false')
  await page.getByText('Complete TAPP', { exact: true }).click()
  await expect(page.locator('[data-admission="widget"]')).toHaveText('true')
  const after = await page.evaluate(() => window.widgetEntranceFixture.coordinator.getConcurrencyStatus().totalAcquired)
  expect(after - before).toBe(1)
})

test('a persistent pending entrance follows the next route gate', async ({ page }) => {
  await page.evaluate(() => window.widgetEntranceFixture.mountAdmissionProbe())
  await expect(page.locator('[data-admission="tapp"]')).toHaveText('false')
  await page.evaluate(() => window.widgetEntranceFixture.coordinator.startPageTransition('next'))
  await expect(page.locator('[data-page-ready]')).toHaveText('false')
  await page.evaluate(() => window.widgetEntranceFixture.coordinator.completePageTransition('next'))
  await expect(page.locator('[data-admission="tapp"]')).toHaveText('true')
  await expect(page.locator('[data-page-ready]')).toHaveText('true')
  await page.evaluate(() => window.widgetEntranceFixture.coordinator.startPageTransition('third'))
  await expect(page.locator('[data-page-ready]')).toHaveText('false')
  await expect(page.locator('[data-admission="tapp"]')).toHaveText('true')
  await page.evaluate(() => window.widgetEntranceFixture.coordinator.completePageTransition('third'))
  await expect(page.locator('[data-admission="widget"]')).toHaveText('true')
})

test('cancelled and previous-route ready callbacks cannot run after dispatch', async ({ page }) => {
  const calls = await page.evaluate(async () => {
    const coordinator = window.widgetEntranceFixture.coordinator
    let calls = 0
    coordinator.startPageTransition('a')
    const off = coordinator.onPageReady(() => calls++)
    coordinator.completePageTransition('a')
    off()
    await Promise.resolve()
    coordinator.startPageTransition('b')
    coordinator.onPageReady(() => calls++)
    coordinator.completePageTransition('b')
    coordinator.startPageTransition('c')
    await Promise.resolve()
    return calls
  })
  expect(calls).toBe(0)
})

test('global widget timers and sizing survive home cleanup and pause while hidden', async ({ page }) => {
  await page.evaluate(() => window.widgetEntranceFixture.mountLifetimeProbe())
  await expect(page.locator('[data-width]')).toHaveText('100')
  await expect(page.locator('[data-loop]')).toHaveText('true')
  await page.getByText('Disable loop', { exact: true }).click()
  await expect(page.locator('[data-loop]')).toHaveText('false')
  await page.evaluate(() => {
    const f = window.widgetEntranceFixture
    f.runPageCleanup('home')
    f.startPage('library')
    document.querySelector<HTMLElement>('[data-resize-target]')!.style.width = '180px'
  })
  await expect(page.locator('[data-width]')).toHaveText('180')
  const before = Number(await page.locator('[data-ticks]').textContent())
  await expect.poll(async () => Number(await page.locator('[data-ticks]').textContent())).toBeGreaterThan(before)
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: true })
    document.dispatchEvent(new Event('visibilitychange'))
  })
  const hidden = await page.locator('[data-ticks]').textContent()
  await page.waitForTimeout(250)
  expect(await page.locator('[data-ticks]').textContent()).toBe(hidden)
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: false })
    document.dispatchEvent(new Event('visibilitychange'))
  })
  await expect.poll(async () => Number(await page.locator('[data-ticks]').textContent())).toBeGreaterThan(Number(hidden))
})

test('background time does not consume entrance delay or admit hidden work', async ({ page }) => {
  const result = await page.evaluate(async () => {
    const c = window.widgetEntranceFixture.coordinator
    const visibility = (hidden: boolean) => {
      Object.defineProperty(document, 'hidden', { configurable: true, value: hidden })
      document.dispatchEvent(new Event('visibilitychange'))
    }
    c.completePageTransition()
    visibility(true)
    c.schedule({ id: 'hidden-delayed', priority: 3, delay: 150 })
    c.schedule({ id: 'hidden-immediate', priority: 3 })
    await new Promise(resolve => setTimeout(resolve, 220))
    const hiddenSlots = c.getConcurrencyStatus().activeSlots
    visibility(false)
    const resumed = c.getState('hidden-delayed')
    const immediate = c.getState('hidden-immediate')
    await new Promise(resolve => setTimeout(resolve, 220))
    const settled = c.getState('hidden-delayed')
    c.markCompleted('hidden-delayed')
    c.markCompleted('hidden-immediate')
    return { hiddenSlots, resumed, immediate, settled }
  })
  expect(result).toEqual({ hiddenSlots: 0, resumed: 'scheduled', immediate: 'ready', settled: 'ready' })
})

test('shared work survives route switches while owner cancellation still wins', async ({ page }) => {
  const calls = await page.evaluate(async () => {
    const f = window.widgetEntranceFixture
    const calls: string[] = []
    f.startPage('home')
    f.batchRead(() => calls.push('read'))
    f.batchWrite(() => calls.push('write'))
    f.scheduleTask(() => calls.push('task'))
    f.coordinator.scheduleIdleTask('global-idle', () => calls.push('idle'))
    const cancel = f.coordinator.scheduleIdleTask('cancelled-idle', () => calls.push('cancelled'))
    cancel()
    f.startPage('library')
    for (let i = 0; i < 120 && calls.length < 4; i++) {
      await new Promise(requestAnimationFrame)
    }
    return calls
  })
  expect(calls.toSorted()).toEqual(['idle', 'read', 'task', 'write'])
  expect(calls.indexOf('read')).toBeLessThan(calls.indexOf('write'))
})
