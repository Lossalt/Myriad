import { expect, test } from '@playwright/test'
import { SITE_FONTS } from '../../src/siteFonts.mjs'

test('document boots before React, removes only the SPA bypass, and loads every local font', async ({ page }) => {
  const remoteFonts: string[] = []
  page.on('request', request => {
    if (/fonts\.(?:googleapis|gstatic)\.com/.test(request.url())) remoteFonts.push(request.url())
  })
  await page.addInitScript(() => localStorage.setItem('theme', 'dark'))
  await page.route('**/api/**', route => {
    const path = new URL(route.request().url()).pathname
    let data: unknown = { success: true, data: [], items: [], sources: [], installations: [] }
    if (path === '/api/setup/status') data = { is_setup_required: false }
    if (path === '/api/auth/me') data = { authenticated: false }
    if (path === '/api/config/ui') data = { dashboard_title: 'Font fixture', dashboard_layout_mode: 'standard' }
    return route.fulfill({ json: data })
  })
  await page.goto('/?_spa=1&keep=yes#anchor')
  await expect(page).toHaveURL(/\/\?keep=yes#anchor$/)
  await expect(page.locator('html')).toHaveClass(/dark/)
  await expect(page.locator('#app-root')).toHaveClass(/app-ready/)
  await expect(page.locator('#app-root')).toHaveCSS('opacity', '1')
  await expect(page.locator('#page-loader')).toHaveAttribute('aria-busy', 'false')
  for (const font of SITE_FONTS) {
    const result = await page.evaluate(async ({ cssVariable, weight }) => {
      const family = getComputedStyle(document.documentElement).getPropertyValue(cssVariable).split(',')[0].trim()
      const faces = await document.fonts.load(`${weight} 16px ${family}`)
      return { count: faces.length, ready: faces.every(face => face.status === 'loaded') }
    }, { cssVariable: font.cssVariable, weight: font.weights[0] })
    expect(result.count, font.name).toBeGreaterThan(0)
    expect(result.ready, font.name).toBe(true)
  }
  const preloads = await page.locator('link[rel="preload"][as="font"]').evaluateAll(links => links.map(link => link.getAttribute('href')))
  expect(preloads).toEqual([SITE_FONTS[0].asset, SITE_FONTS[1].asset])
  expect(remoteFonts).toEqual([])
})

test('deep links share one document and missing assets remain 404', async ({ request }) => {
  const home = await request.get('/')
  const shell = await home.text()
  for (const path of ['/studio/notes', '/tapp/run/abc', '/agent/settings', '/config']) {
    const response = await request.get(path)
    expect(response.status()).toBe(200)
    expect(await response.text()).toBe(shell)
    expect(response.headers()['permissions-policy']).toContain('microphone=(self)')
    expect(response.headers()['cache-control']).toBe('no-cache')
  }
  expect((await request.get('/assets/missing.js')).status()).toBe(404)
  const font = await request.get(SITE_FONTS[0].asset)
  expect(font.headers()['cache-control']).toContain('immutable')
  expect(font.headers()['content-type']).toBe('font/woff2')
  expect((await request.get('/sw.js')).headers()['cache-control']).toBe('no-cache')
})

test('no-JavaScript document exposes the fallback and hides the loading screen', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false })
  try {
    const page = await context.newPage()
    await page.goto('http://127.0.0.1:4188/')
    await expect(page.locator('#noscript-enable-js')).toBeVisible()
    await expect(page.locator('#page-loader')).toBeHidden()
  } finally { await context.close() }
})

test('keeps the document loader and defers background startup until the route commits', async ({ page }) => {
  const backgroundRequests: string[] = []
  page.on('request', request => {
    if (/\/TappBackgroundRunner-[^/]+\.js$/.test(request.url())) backgroundRequests.push(request.url())
  })
  await page.route('**/api/**', route => {
    const path = new URL(route.request().url()).pathname
    return route.fulfill({ json: path === '/api/setup/status' ? { is_setup_required: false } : path === '/api/auth/me' ? { authenticated: false } : { success: true, data: [], items: [], installations: [] } })
  })
  let release!: () => void
  const gate = new Promise<void>(resolve => { release = resolve })
  let requested!: () => void
  const routeRequested = new Promise<void>(resolve => { requested = resolve })
  await page.route('**/assets/Home-*.js', async route => {
    requested()
    await gate
    await route.continue()
  })
  try {
    await page.goto('/', { waitUntil: 'domcontentloaded' })
    await routeRequested
    // Exceed the background host's three-second delay as well as the loader frames.
    await page.waitForTimeout(4200)
    expect(backgroundRequests).toEqual([])
    await expect(page.locator('#page-loader')).toHaveAttribute('aria-busy', 'true')
    await expect(page.locator('#app-root')).not.toHaveClass(/app-ready/)
    release()
    await expect(page.locator('#app-root')).toHaveClass(/app-ready/)
    await expect(page.locator('#page-loader')).toHaveAttribute('aria-busy', 'false')
  } finally { release() }
})

test('reveals route recovery when the initial page module fails', async ({ page }) => {
  await page.route('**/api/**', route => route.fulfill({ json: { is_setup_required: false, authenticated: false, data: [] } }))
  await page.route('**/assets/Home-*.js', route => route.abort())
  await page.goto('/')
  await expect(page.locator('#app-root')).toHaveClass(/app-ready/)
  await expect(page.locator('[role="alert"]').first()).toBeVisible()
  await expect(page.locator('#page-loader')).toHaveAttribute('aria-busy', 'false')
})

test('offers visible recovery if both the chosen and fallback locale fail', async ({ page }) => {
  await page.route(/\/assets\/(?:zh-CN|en-US)-[^/]+\.js$/, route => route.abort())
  await page.addInitScript(() => localStorage.setItem('locale', 'zh-CN'))
  await page.goto('/')
  await expect(page.getByRole('button', { name: 'Reload', exact: true })).toBeVisible()
  await expect(page.locator('#page-loader')).toHaveAttribute('aria-busy', 'false')
})

test('home shares the provider session probe while the first response is pending', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('myriad_session_hint', 'true'))
  let probes = 0
  let release!: () => void
  const gate = new Promise<void>(resolve => { release = resolve })
  await page.route('**/api/**', async route => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/auth/me') {
      probes++
      await gate
      return route.fulfill({ json: { authenticated: true, id: 1, username: 'reader', is_admin: false } })
    }
    return route.fulfill({ json: path === '/api/setup/status' ? { is_setup_required: false } : { success: true, data: [], items: [], installations: [] } })
  })
  try {
    await page.goto('/', { waitUntil: 'domcontentloaded' })
    await expect(page.locator('#app-root')).toHaveClass(/app-ready/)
    expect(probes).toBe(1)
    const response = page.waitForResponse('**/api/auth/me')
    release()
    await response
    // Allow state effects and any incorrectly queued second probe to execute.
    await page.waitForTimeout(500)
    expect(probes).toBe(1)
  } finally { release() }
})
