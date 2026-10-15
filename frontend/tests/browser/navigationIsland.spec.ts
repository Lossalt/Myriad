import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

for (const width of [390, 1440]) {
  for (const action of ['back', 'escape']) {
    test(`first ${action} after library subroute stays in primary menu at ${width}px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 })
      await page.route('**/api/**', route => route.fulfill({ json: {} }))
      await page.route('**/library', route => {
        if (new URL(route.request().url()).pathname !== '/library') return route.fallback()
        return route.fulfill({
          contentType: 'text/html',
          body: '<!doctype html><meta charset="utf-8"><div id="root"></div>',
        })
      })
      await page.goto('/library')
      const fixture = `/@fs${fileURLToPath(new URL('./fixture/navigationIsland.tsx', import.meta.url))}`
      await page.evaluate(async path => {
        const { mountNavigationIsland } = await import(path)
        mountNavigationIsland()
      }, fixture)
      const back = page.locator('[data-group="back"] button')
      const island = page.locator('.dynamic-island')
      await expect(back).toBeVisible()
      await expect(island).not.toHaveAttribute('data-transitioning')
      await page.getByRole('button', { name: 'Open shelf' }).click()
      await expect(page).toHaveURL(/\/library\/canvas$/)
      await expect(island).not.toHaveAttribute('data-transitioning')
      if (action === 'back') { await back.click()
}
      else {
        await back.focus()
        await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))))
        await page.keyboard.press('Escape')
      }
      await expect(back).toHaveCount(0)
      await expect(island).not.toHaveAttribute('data-transitioning')
      // Cross the delayed auto-expand window after the collapse finishes.
      await page.waitForTimeout(650)
      await expect(back).toHaveCount(0)
      await page.locator('[data-group="library"] button').click()
      await expect(back).toBeVisible()
    })
  }
}

test('navigation chrome follows repeated mobile and desktop viewport changes', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 900 })
  await page.route('**/api/**', route => route.fulfill({ json: {} }))
  await page.route('**/library', route => {
    if (new URL(route.request().url()).pathname !== '/library') return route.fallback()
    return route.fulfill({
      contentType: 'text/html',
      body: '<!doctype html><meta charset="utf-8"><div id="root"></div>',
    })
  })
  await page.goto('/library')
  const fixture = `/@fs${fileURLToPath(new URL('./fixture/navigationIsland.tsx', import.meta.url))}`
  await page.evaluate(async path => {
    const { mountNavigationIsland } = await import(path)
    mountNavigationIsland()
  }, fixture)
  await expect(page.locator('.dynamic-island')).toBeVisible()
  for (const width of [1440, 390, 1440]) {
    await page.setViewportSize({ width, height: 900 })
    await expect(page.locator('html')).toHaveAttribute('data-nav-layout', width === 390 ? 'mobile' : 'desktop')
    await expect(page.locator('.nav-container')).not.toHaveAttribute('data-nav-switch')
    await expect(page.locator('.dynamic-island')).toBeVisible()
  }
})
