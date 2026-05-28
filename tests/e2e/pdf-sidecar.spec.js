import { expect, test } from '@playwright/test'

test('PDF sidecar UI opens immediately and shows the full mono artifact after completion', async ({ page }) => {
  await page.goto('/?harness=pdf-sidecar&scenario=complete')

  await page.getByTestId('translation-action').click()

  await expect(page.getByTestId('view-mode-dual')).toHaveClass(/active/)
  await expect(page.getByTestId('pdf-sidecar-start-calls')).toHaveText('1')
  await expect(page.getByTestId('pdf-sidecar-translation-context')).toContainText('"visiblePages":[1]')

  await expect(page.getByTestId('pdf-sidecar-partial-history')).toContainText('1')
  await expect(page.getByTestId('pdf-sidecar-status')).toContainText('succeeded|finished|full')
  await expect(page.getByTestId('translated-artifact-viewer')).toBeVisible()
  await expect(page.locator('[data-testid="pdf-page"][data-page="1"]').first()).toBeVisible()
  await expect(page.getByTestId('translation-artifact-label')).toContainText('finished')
})

test('PDF sidecar controls sync zoom across dual PDF panes', async ({ page }) => {
  await page.goto('/?harness=pdf-sidecar&scenario=complete')

  await page.getByTestId('translation-action').click()
  await expect(page.getByTestId('translated-artifact-viewer')).toBeVisible()
  await expect(page.locator('.translation-progress-dock')).toHaveCount(0)

  const sourcePage = page.locator('.local-pdf-source [data-testid="pdf-page"][data-page="1"]').first()
  const translatedPage = page.locator('.translated-reader [data-testid="pdf-page"][data-page="1"]').first()
  await expect(sourcePage).toBeVisible()
  await expect(translatedPage).toBeVisible()

  const before = await Promise.all([
    sourcePage.evaluate((el) => el.getBoundingClientRect().width),
    translatedPage.evaluate((el) => el.getBoundingClientRect().width),
  ])

  await page.getByRole('button', { name: 'Zoom in' }).click()
  await expect.poll(async () => sourcePage.evaluate((el) => el.getBoundingClientRect().width))
    .toBeGreaterThan(before[0])
  await expect.poll(async () => translatedPage.evaluate((el) => el.getBoundingClientRect().width))
    .toBeGreaterThan(before[1])

  const after = await Promise.all([
    sourcePage.evaluate((el) => el.getBoundingClientRect().width),
    translatedPage.evaluate((el) => el.getBoundingClientRect().width),
  ])
  expect(Math.abs(after[0] - after[1])).toBeLessThanOrEqual(1)

  await expect(page.getByRole('button', { name: 'Next' })).toBeDisabled()
})

test('PDF sidecar scroll lock syncs translated PDF scrolling back to the source pane', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 520 })
  await page.goto('/?harness=pdf-sidecar&scenario=complete')

  await page.getByTestId('translation-action').click()
  await expect(page.getByTestId('translated-artifact-viewer')).toBeVisible()

  const linkButton = page.getByTestId('pane-scroll-link-toggle')
  await expect(linkButton).toBeEnabled()
  await linkButton.click()
  await expect(linkButton).toHaveAttribute('aria-pressed', 'true')

  const sourceScroll = page.locator('.local-pdf-source .pdf-scroll').first()
  const translatedScroll = page.locator('.translated-reader .pdf-scroll').first()
  await expect.poll(async () => translatedScroll.evaluate((el) => el.scrollHeight > el.clientHeight))
    .toBe(true)
  await translatedScroll.evaluate((el) => {
    el.scrollTop = 240
    el.dispatchEvent(new Event('scroll', { bubbles: true }))
  })

  await expect.poll(async () => sourceScroll.evaluate((el) => el.scrollTop))
    .toBeGreaterThan(80)

  await translatedScroll.evaluate((el) => {
    for (const top of [500, 430, 360, 280]) {
      el.scrollTop = top
      el.dispatchEvent(new Event('scroll', { bubbles: true }))
    }
  })
  await expect.poll(async () => sourceScroll.evaluate((el) => el.scrollTop))
    .toBeGreaterThan(180)

  await translatedScroll.evaluate((el) => {
    el.scrollTop = 45
    el.dispatchEvent(new Event('scroll', { bubbles: true }))
  })
  await expect.poll(async () => sourceScroll.evaluate((el) => el.scrollTop))
    .toBeLessThan(120)
})

test('PDF sidecar UI can cancel an in-progress partial translation', async ({ page }) => {
  await page.goto('/?harness=pdf-sidecar&scenario=cancel')

  await page.getByTestId('translation-action').click()
  await expect(page.getByTestId('cancel-translation')).toBeVisible()

  await page.getByTestId('cancel-translation').click()

  await expect(page.getByTestId('pdf-sidecar-cancel-calls')).toHaveText('1')
  await expect(page.getByTestId('pdf-sidecar-status')).toContainText('canceled|canceled|partial|1')
})

test('PDF sidecar UI keeps the translation pane anchored when later partial pages finish', async ({ page }) => {
  await page.goto('/?harness=pdf-sidecar&scenario=multi-partial')

  await page.getByTestId('translation-action').click()

  await expect(page.getByTestId('pdf-sidecar-partial-history')).toHaveText('1,2')
  await expect(page.getByTestId('pdf-sidecar-status')).toContainText('partial|partial_ready|partial|2')
  await expect(page.getByTestId('translated-artifact-viewer')).toBeVisible()
  await expect(page.getByTestId('translation-artifact-label')).toContainText('Partial p1')
})

test('PDF sidecar UI shows provider errors and retries successfully', async ({ page }) => {
  await page.goto('/?harness=pdf-sidecar&scenario=error-retry')

  await page.getByTestId('translation-action').click()
  await expect(page.getByTestId('translation-full-state')).toContainText('Unsupported PDF translation provider')
  await expect(page.getByTestId('pdf-sidecar-status')).toContainText('failed|failed')
  await expect(page.getByTestId('translation-full-state')).toBeVisible()

  await page.getByTestId('translation-action').click()

  await expect(page.getByTestId('pdf-sidecar-start-calls')).toHaveText('2')
  await expect(page.getByTestId('translated-artifact-viewer')).toBeVisible()
  await expect(page.getByTestId('pdf-sidecar-status')).toContainText('succeeded|finished|full')
  await expect(page.getByTestId('translated-artifact-viewer')).toBeVisible()
})
