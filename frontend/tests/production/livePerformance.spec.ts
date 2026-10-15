import { writeFile } from 'node:fs/promises'
import process from 'node:process'
import { expect, test } from '@playwright/test'

test('real public data remains responsive across repeated page visits', async ({ page, browserName }, testInfo) => {
  const apiOrigin = process.env.MYRIAD_PERF_API_ORIGIN
  test.skip(!apiOrigin, 'Requires an explicit real backend; run test:live-performance')
  const errors: string[] = []
  const failedReads = new Map<string, number>()
  const recordFailure = (key: string) => failedReads.set(key, (failedReads.get(key) ?? 0) + 1)
  const inventory = new Map<string, number>()
  page.on('pageerror', error => errors.push(error.message))
  await page.route('**/api/**', async (route) => {
    const request = route.request()
    // Exercise the real anonymous read surface without modifying the user's data.
    if (!['GET', 'HEAD'].includes(request.method())) {
      await route.fulfill({ status: 405, contentType: 'application/json', body: '{"success":false}' })
      return
    }
    const url = new URL(request.url())
    try {
      const response = await route.fetch({ url: `${apiOrigin}${url.pathname}${url.search}`, timeout: 30_000 })
      if (!response.ok()) recordFailure(`${response.status()} ${url.pathname}`)
      if (response.ok() && response.headers()['content-type']?.includes('json')) {
        const body = await response.json()
        for (const key of ['sources', 'items']) {
          if (!Array.isArray(body[key])) continue
          const name = `${url.pathname}:${key}`
          inventory.set(name, Math.max(inventory.get(name) ?? 0, body[key].length))
        }
      }
      await route.fulfill({ response })
    } catch (error) {
      recordFailure(`${url.pathname}: ${String(error)}`)
      await route.abort()
    }
  })
  await page.addInitScript((origin) => {
    const metrics = {
      supported: PerformanceObserver.supportedEntryTypes,
      lcp: null as number | null,
      maxEventDuration: null as number | null,
      longTasks: PerformanceObserver.supportedEntryTypes.includes('longtask') ? [] as Array<{ start: number; duration: number }> : null,
      cyclesStarted: 0,
      sockets: 0,
      events: [] as Array<{ name: string; duration: number; inputDelay: number; processing: number }>,
    }
    ;(window as any).__livePerformance = metrics
    for (const type of ['longtask', 'largest-contentful-paint', 'event']) {
      if (!metrics.supported.includes(type)) continue
      new PerformanceObserver(list => {
        for (const entry of list.getEntries()) {
          if (type === 'longtask') { metrics.longTasks!.push({ start: entry.startTime, duration: entry.duration })
}
          else if (type === 'event') {
            const event = entry as PerformanceEventTiming
            metrics.maxEventDuration = Math.max(metrics.maxEventDuration ?? 0, entry.duration)
            metrics.events.push({ name: event.name, duration: event.duration, inputDelay: event.processingStart - event.startTime, processing: event.processingEnd - event.processingStart })
            if (metrics.events.length > 100) metrics.events.shift()
          }
          else { metrics.lcp = entry.startTime
}
        }
      }).observe({ type, buffered: true, durationThreshold: 16 })
    }
    const Original = window.WebSocket
    window.WebSocket = class extends Original {
      constructor(url: string | URL, protocols?: string | string[]) {
        const target = new URL(url, location.href)
        if (target.pathname.startsWith('/api/')) {
          const backend = new URL(origin!)
          target.host = backend.host
          target.protocol = backend.protocol === 'https:' ? 'wss:' : 'ws:'
        }
        super(target, protocols)
        metrics.sockets++
        this.addEventListener('close', () => metrics.sockets--, { once: true })
      }
    }
  }, apiOrigin)
  await page.goto('/')
  await expect(page.locator('.home-shell__inner')).toBeVisible({ timeout: 30_000 })
  await page.evaluate(() => { (window as any).__livePerformance.cyclesStarted = performance.now() })
  const cdp = browserName === 'chromium' ? await page.context().newCDPSession(page) : null
  const samples: Array<{ cycle: number; nodes: number; sockets: number; retainedHeap: number | null }> = []
  const inputLatency: number[] = []
  const forcedGcWindows: Array<{ start: number; end: number }> = []
  for (let cycle = 0; cycle < 12; cycle++) {
    await page.evaluate(() => { history.pushState(null, '', '/library'); dispatchEvent(new PopStateEvent('popstate')) })
    await expect(page.locator('.library-card-container').first()).toBeVisible({ timeout: 20_000 })
    await expect.poll(() => page.locator('.library-card-container').count()).toBeGreaterThan(0)
    const rail = page.locator('.library-card-container').first()
    await rail.hover()
    const started = Date.now()
    await page.mouse.wheel(500, 0)
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))))
    inputLatency.push(Date.now() - started)
    await page.evaluate(() => { history.pushState(null, '', '/'); dispatchEvent(new PopStateEvent('popstate')) })
    await expect(page.locator('.home-shell__inner')).toBeVisible()
    await page.locator('nav a[href="/"]').first().focus()
    await page.keyboard.press('Enter')
    // Finish the input paint before forcing GC; GC must not inflate Event Timing.
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))))
    await page.waitForTimeout(500)
    if (cdp) {
      const start = await page.evaluate(() => performance.now())
      await cdp.send('HeapProfiler.collectGarbage')
      forcedGcWindows.push({ start, end: await page.evaluate(() => performance.now()) })
    }
    const state = await page.evaluate(() => ({ nodes: document.querySelectorAll('*').length, sockets: (window as any).__livePerformance.sockets }))
    samples.push({ cycle, ...state, retainedHeap: cdp ? (await cdp.send('Runtime.getHeapUsage')).usedSize : null })
  }
  const metrics = await page.evaluate(() => (window as any).__livePerformance)
  // Retain raw entries, and mark tasks overlapping the invasive heap probe.
  metrics.longTasks = metrics.longTasks?.map((task: { start: number; duration: number }) => ({
    ...task, overlapsForcedGc: forcedGcWindows.some(window => task.start <= window.end && task.start + task.duration >= window.start),
  })) ?? null
  const report = testInfo.outputPath('live-performance.json')
  await writeFile(report, JSON.stringify({ browser: browserName, data: Object.fromEntries(inventory), metrics, forcedGcWindows, inputToTwoFramesMs: inputLatency, samples, pageErrors: errors, failedReads: Object.fromEntries(failedReads) }, null, 2))
  await testInfo.attach('live-performance', { path: report, contentType: 'application/json' })
  expect(errors).toEqual([])
  expect(inventory.get('/api/library:items')).toBeGreaterThan(0)
  expect([...failedReads.keys()].filter(value => /\/api\/library(?:$|:)/.test(value))).toEqual([])
  expect(samples.at(-1)!.sockets).toBeLessThanOrEqual(1)
  expect(samples.at(-1)!.nodes).toBeLessThanOrEqual(samples[1].nodes + 100)
  if (cdp) expect(samples.at(-1)!.retainedHeap! - samples[1].retainedHeap!).toBeLessThan(12 * 1024 * 1024)
  expect(Math.max(...inputLatency)).toBeLessThan(2000)
  if (metrics.lcp !== null) expect(metrics.lcp).toBeLessThan(10_000)
  if (metrics.maxEventDuration !== null) expect(metrics.maxEventDuration).toBeLessThan(1000)
  if (metrics.longTasks) {
    const interactive = metrics.longTasks
      .filter((task: { start: number; duration: number; overlapsForcedGc?: boolean }) =>
        !task.overlapsForcedGc && task.start >= metrics.cyclesStarted)
      .map((task: { duration: number }) => task.duration)
    expect(Math.max(0, ...interactive)).toBeLessThan(500)
  }
})
