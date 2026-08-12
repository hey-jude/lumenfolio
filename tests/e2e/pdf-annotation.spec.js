import { expect, test } from '@playwright/test'

const tool = (page, name) => page.getByRole('button', { name, exact: true })

async function openAnnotationHarness(page) {
  await page.goto('/?harness=pdf-annotation')
  await expect(page.locator('.annotationEditorLayer').first()).toBeVisible()
  await expect(tool(page, 'Select')).toBeEnabled()
  await expect(tool(page, 'Select')).toHaveAttribute('aria-pressed', 'true')
}

async function switchTool(page, name) {
  await tool(page, name).click()
  await expect(tool(page, name)).toHaveAttribute('aria-pressed', 'true')
}

test('PDF annotation tools switch repeatedly without leaving editor layers stale', async ({ page }) => {
  await openAnnotationHarness(page)

  for (const name of ['Highlight', 'Text', 'Draw', 'Select', 'Text', 'Highlight', 'Select']) {
    await switchTool(page, name)
  }

  // Do not wait for each mode-change event. The last click must win even while
  // PDF.js is still completing an earlier transition.
  await tool(page, 'Text').click()
  await tool(page, 'Draw').click()
  await tool(page, 'Highlight').click()
  await tool(page, 'Select').click()
  await expect(tool(page, 'Select')).toHaveAttribute('aria-pressed', 'true')

  const layer = page.locator('.annotationEditorLayer').first()
  await expect(layer).toHaveClass(/highlightEditing/)
  await expect(page.locator('.textLayer').first()).toHaveCSS('pointer-events', 'none')
})

test('Highlight marks selectable PDF text after switching from every other tool', async ({ page }) => {
  await openAnnotationHarness(page)

  for (const precedingTool of ['Text', 'Draw', 'Select']) {
    await switchTool(page, precedingTool)
    await switchTool(page, 'Highlight')

    const text = page.locator('.textLayer span').filter({ hasText: 'Goal Hint Generation' }).first()
    await expect(text).toBeVisible()
    const box = await text.boundingBox()
    expect(box).toBeTruthy()
    await page.mouse.move(box.x + 2, box.y + box.height / 2)
    await page.mouse.down()
    await page.mouse.move(box.x + Math.max(10, box.width - 2), box.y + box.height / 2, { steps: 8 })
    await page.mouse.up()

    await expect(page.locator('.highlightEditor')).toHaveCount(1)
    await page.getByRole('button', { name: 'Undo', exact: true }).click()
    await expect(page.locator('.highlightEditor')).toHaveCount(0)
  }

  // Pages without a usable text layer (for example scanned PDFs) must still
  // accept PDF.js free highlighting on the page background.
  const textLayer = page.locator('.textLayer').first()
  const freeStroke = await textLayer.evaluate((layer) => {
    const rect = layer.getBoundingClientRect()
    for (let y = rect.top + 40; y < rect.bottom - 40; y += 20) {
      for (let x = rect.left + 40; x < rect.right - 180; x += 20) {
        if (document.elementFromPoint(x, y) === layer
          && document.elementFromPoint(x + 120, y + 20) === layer) {
          return { x, y }
        }
      }
    }
    return null
  })
  expect(freeStroke).toBeTruthy()
  await page.mouse.move(freeStroke.x, freeStroke.y)
  await page.mouse.down()
  await page.mouse.move(freeStroke.x + 120, freeStroke.y + 20, { steps: 12 })
  await page.mouse.up()
  await expect(page.locator('.highlightEditor.free')).toHaveCount(1)
})

test('FreeText and Ink still receive page pointer input after repeated switching', async ({ page }) => {
  await openAnnotationHarness(page)
  const layer = page.locator('.annotationEditorLayer').first()
  const box = await layer.boundingBox()
  expect(box).toBeTruthy()

  await switchTool(page, 'Text')
  await page.mouse.click(box.x + 120, box.y + 150)
  const editor = page.locator('.freeTextEditor [contenteditable="true"]').first()
  await expect(editor).toBeVisible()
  await editor.fill('annotation text')

  await switchTool(page, 'Select')
  await switchTool(page, 'Text')
  await page.locator('.freeTextEditor').first().dblclick()
  await expect(editor).toBeFocused()
  await editor.press('End')
  await editor.pressSequentially(' updated')
  await expect(editor).toContainText('annotation text updated')

  await switchTool(page, 'Draw')
  await page.mouse.move(box.x + 160, box.y + 210)
  await page.mouse.down()
  await page.mouse.move(box.x + 240, box.y + 250, { steps: 12 })
  await page.mouse.up()

  await switchTool(page, 'Select')
  await expect(page.locator('.freeTextEditor')).toHaveCount(1)
  await expect(page.locator('.inkEditor')).toHaveCount(1)

  await page.locator('.freeTextEditor').first().click()
  await expect(page.locator('.freeTextEditor').first()).toHaveClass(/selectedEditor/)
  await expect(page.getByRole('button', { name: 'Erase', exact: true })).toBeEnabled()
})

test('Save locks annotation input and keeps the saved baseline clean', async ({ page }) => {
  await openAnnotationHarness(page)
  const layer = page.locator('.annotationEditorLayer').first()
  const box = await layer.boundingBox()
  expect(box).toBeTruthy()

  await switchTool(page, 'Text')
  await page.mouse.click(box.x + 120, box.y + 150)
  const editor = page.locator('.freeTextEditor [contenteditable="true"]').first()
  await editor.fill('saved annotation')
  await expect(page.locator('.annotation-status')).toContainText('Unsaved changes')

  await page.evaluate(() => {
    window.__pdfAnnotationSaveDelay = 350
  })
  await page.getByRole('button', { name: 'Save', exact: true }).click()
  await expect(page.locator('.annotation-status')).toContainText('Saving')
  await expect(tool(page, 'Select')).toBeDisabled()
  await expect(page.locator('.annotation-pdf-container')).toHaveAttribute('inert', '')
  await expect(page.locator('.annotation-status')).toContainText('Saved')

  await switchTool(page, 'Select')
  await page.locator('.freeTextEditor').first().click()
  await expect(page.locator('.annotation-status')).toContainText('Saved')
  const saves = await page.evaluate(() => window.__pdfAnnotationInvokes.filter(({ command }) => command === 'save_pdf_document'))
  expect(saves.at(-1)).toMatchObject({ documentId: 'e2e-pdf-annotation' })
  expect(saves.at(-1).byteLength).toBeGreaterThan(0)
})

test('A save finishing after document replacement cannot target or update the new PDF', async ({ page }) => {
  await openAnnotationHarness(page)
  await page.evaluate(() => {
    window.__pdfAnnotationSaveDelay = 500
  })

  await page.getByRole('button', { name: 'Save', exact: true }).click()
  await expect(page.locator('.annotation-status')).toContainText('Saving')
  await page.getByRole('button', { name: 'Switch document', exact: true }).click()
  await expect(page.locator('.annotationEditorLayer').first()).toBeVisible()
  await expect(tool(page, 'Select')).toBeEnabled()
  await page.waitForTimeout(550)

  const invokes = await page.evaluate(() => window.__pdfAnnotationInvokes)
  const saves = invokes.filter(({ command }) => command === 'save_pdf_document')
  expect(saves).toHaveLength(1)
  expect(saves[0].documentId).toBe('e2e-pdf-annotation')
  expect(invokes.some(({ command, docId }) => command === 'read_pdf_bytes' && docId === 'e2e-pdf-annotation-next')).toBeTruthy()
  await expect(page.locator('.annotation-status')).toContainText('Saved')
})
