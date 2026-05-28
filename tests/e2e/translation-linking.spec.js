import { expect, test } from '@playwright/test'

test('translation view lazy-loads page translations without PDF artifacts', async ({ page }) => {
  await page.goto('/?harness=translation-linking')

  await expect(page.getByTestId('view-mode-dual')).toHaveClass(/active/)
  await expect(page.getByTestId('pdf-page')).toBeVisible()
  await expect(page.locator('[data-testid="translation-page"][data-page="1"]')).toBeVisible()
  await expect(page.locator('[data-testid="translation-page"][data-page="2"]')).toBeVisible()
  await expect(page.locator('[data-testid="translation-page"][data-page="3"]')).toBeVisible()
  await expect(page.locator('[data-testid="translation-block"][data-page="1"]').first()).toBeVisible()
  await expect(page.locator('[data-testid="translation-placeholder"][data-page="4"]')).toContainText('Loading page')
  await expect(page.getByTestId('requested-translation-pages')).toContainText('4')
  await page.waitForTimeout(300)
  const requestedPages = ((await page.getByTestId('requested-translation-pages').textContent()) || '')
    .split(',')
    .map((value) => Number(value))
    .filter(Boolean)
  const requestCounts = requestedPages.reduce((counts, pageNo) => {
    counts.set(pageNo, (counts.get(pageNo) || 0) + 1)
    return counts
  }, new Map())
  expect(Math.max(...requestCounts.values())).toBe(1)
})

test('translated text blocks link back to source highlights', async ({ page }) => {
  await page.goto('/?harness=translation-linking')

  await expect(page.getByTestId('view-mode-dual')).toHaveClass(/active/)

  const neuralSkimmerBlock = page.locator('[data-testid="translation-block"][data-block-id="p1-b2"]').first()
  await expect(neuralSkimmerBlock).toBeVisible()

  await neuralSkimmerBlock.hover()
  await expect(page.locator('[data-testid="reader-highlight-linked"][data-page="1"]')).toBeVisible()

  await neuralSkimmerBlock.click()
  await expect(page.locator('[data-testid="reader-highlight-active"][data-page="1"]').first()).toBeVisible()
})

test('translation pane scrolling updates the active page anchor when panes are linked', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 520 })
  await page.goto('/?harness=translation-linking')

  await expect(page.getByTestId('view-mode-dual')).toHaveClass(/active/)
  await expect(page.getByTestId('active-page')).toHaveText('1')
  await expect(page.getByTestId('pane-scroll-link-toggle')).toHaveAttribute('data-linked', 'false')
  await page.waitForTimeout(250)

  await page.locator('.translation-page').evaluate((scroller) => {
    const targetPage = scroller.querySelector('[data-testid="translation-page"][data-page="8"]')
    const targetTop = targetPage
      ? targetPage.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop
      : 0
    scroller.scrollTop = targetTop
    scroller.dispatchEvent(new Event('scroll'))
  })

  await expect(page.getByTestId('active-page')).toHaveText('1')

  await page.getByTestId('pane-scroll-link-toggle').click()
  await expect(page.getByTestId('pane-scroll-link-toggle')).toHaveAttribute('data-linked', 'true')
  await page.waitForTimeout(500)
  await page.locator('.translation-page').evaluate((scroller) => {
    const targetPage = scroller.querySelector('[data-testid="translation-page"][data-page="8"]')
    const targetTop = targetPage
      ? targetPage.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop
      : 0
    scroller.scrollTop = targetTop
    scroller.dispatchEvent(new Event('scroll'))
  })

  await expect(page.getByTestId('active-page')).toHaveText(/^[89]$/)
})

test('translation pane scroll lock preference persists across reloads', async ({ page }) => {
  await page.goto('/?harness=translation-linking')

  await expect(page.getByTestId('pane-scroll-link-toggle')).toHaveAttribute('data-linked', 'false')
  await page.getByTestId('pane-scroll-link-toggle').click()
  await expect(page.getByTestId('pane-scroll-link-toggle')).toHaveAttribute('data-linked', 'true')

  await page.reload()

  await expect(page.getByTestId('view-mode-dual')).toHaveClass(/active/)
  await expect(page.getByTestId('pane-scroll-link-toggle')).toHaveAttribute('data-linked', 'true')
})
