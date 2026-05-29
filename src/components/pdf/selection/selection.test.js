// Unit tests for the custom selection geometry kernel.
// Run with: node --test src/components/pdf/selection/
//
// No DOM, no PDF — glyph fixtures are hand-built page-local pixel rects so the
// reading-order logic can be verified deterministically.

import test from 'node:test'
import assert from 'node:assert/strict'

import {
  buildSelection,
  computePageColumns,
  columnIndexAt,
  clipGlyphsToSelection,
  glyphsToText,
  glyphAtPoint,
  mergeGlyphsToLineRects,
} from './index.js'

// --- fixture helpers --------------------------------------------------------

const PAGE = { width: 600, height: 800 }
const GLYPH_W = 7 // per-character width
const LINE_H = 12

// Build a glyph (one word) at a grid position.
function word(text, x, y) {
  return { text, x, y, width: text.length * GLYPH_W, height: LINE_H }
}

// A line of words starting at x, spaced by one space-width.
function line(words, x, y) {
  const glyphs = []
  let cx = x
  for (const w of words) {
    glyphs.push(word(w, cx, y))
    cx += w.length * GLYPH_W + GLYPH_W // word + one space
  }
  return glyphs
}

// Emit a densely-filled run of word glyphs from x=left to x=right at row y.
// Real PDF text packs words tightly, so the only wide horizontal gap on a page
// is the column gutter — never an inter-word space. The earlier sparse fixtures
// (4-5 short words per line) created word-center gaps as wide as a real gutter,
// which is unrealistic; this mirrors actual text density.
// Emit CHARACTER-level glyphs filling x=left..right at row y — matching the
// real pipeline (splitItemsIntoCharGlyphs), where each glyph is one character.
// Characters tile the row contiguously; the only wide horizontal gap on a page
// is the column gutter.
function denseRow(left, right, y, tag) {
  const glyphs = []
  let cx = left
  let i = 0
  while (cx + GLYPH_W <= right) {
    glyphs.push({ text: tag[i % tag.length] || 'x', x: cx, y, width: GLYPH_W, height: LINE_H })
    cx += GLYPH_W
    i += 1
  }
  return glyphs
}

// Two-column page (722px wide): left column 40..330, gutter ~330..392, right
// column 392..680. Optional full-width abstract band at the top.
const PAGE2 = { width: 722, height: 935 }
const GUTTER_MID = 361 // halfway across the gutter
function twoColumnPage({ withAbstract = false } = {}) {
  const glyphs = []
  let y = 40
  if (withAbstract) {
    // Full-width line spanning across both columns (40..680).
    glyphs.push(...denseRow(40, 680, y, 'A'))
    y += LINE_H * 2
  }
  const colTop = y
  for (let i = 0; i < 5; i += 1) {
    glyphs.push(...denseRow(40, 330, colTop + i * LINE_H, 'L'))
    glyphs.push(...denseRow(392, 680, colTop + i * LINE_H, 'R'))
  }
  return { glyphs, colTop }
}

function singleColumnPage() {
  const glyphs = []
  for (let i = 0; i < 6; i += 1) {
    glyphs.push(...denseRow(40, 560, 40 + i * LINE_H, 'w'))
  }
  return { glyphs }
}

// --- column detection -------------------------------------------------------

test('single-column page yields one band', () => {
  const { glyphs } = singleColumnPage()
  const cols = computePageColumns(glyphs, PAGE)
  assert.equal(cols.bands.length, 1)
})

test('two-column page yields two bands with a gutter between them', () => {
  const { glyphs } = twoColumnPage()
  const cols = computePageColumns(glyphs, PAGE2)
  assert.equal(cols.bands.length, 2, JSON.stringify(cols.bands))
  assert.ok(cols.bands[0].right <= cols.bands[1].left + 1, 'bands should not overlap')
})

test('full-width abstract is detected and excluded from column split', () => {
  const { glyphs } = twoColumnPage({ withAbstract: true })
  const cols = computePageColumns(glyphs, PAGE2)
  assert.equal(cols.bands.length, 2, 'body still two columns')
  assert.ok(cols.fullWidth.length >= 1, 'abstract recorded as full-width region')
})

test('columnIndexAt maps x to the right band and clamps gutter points', () => {
  const { glyphs } = twoColumnPage()
  const cols = computePageColumns(glyphs, PAGE2)
  assert.equal(columnIndexAt(60, cols.bands), 0)
  assert.equal(columnIndexAt(500, cols.bands), 1)
})

// Helper: which column a glyph's center belongs to.
const colOf = (g, cols) => columnIndexAt(g.x + g.width / 2, cols.bands)

// --- the core bug: drag in left column must not leak into right -------------

test('dragging down the left column selects only left-column glyphs', () => {
  const { glyphs, colTop } = twoColumnPage()
  const cols = computePageColumns(glyphs, PAGE2)
  const start = { x: 45, y: colTop + 1 }
  const end = { x: 200, y: colTop + 4 * LINE_H + 1 }
  const selected = clipGlyphsToSelection(glyphs, cols, start, end)
  assert.ok(selected.length > 0)
  assert.ok(selected.every((g) => colOf(g, cols) === 0),
    `leaked into right column: maxRight=${Math.max(...selected.map((g) => g.x + g.width))}`)
})

test('dragging down the right column selects only right-column glyphs', () => {
  const { glyphs, colTop } = twoColumnPage()
  const cols = computePageColumns(glyphs, PAGE2)
  const start = { x: 400, y: colTop + 1 }
  const end = { x: 600, y: colTop + 4 * LINE_H + 1 }
  const selected = clipGlyphsToSelection(glyphs, cols, start, end)
  assert.ok(selected.length > 0)
  assert.ok(selected.every((g) => colOf(g, cols) === 1),
    `leaked into left column: minLeft=${Math.min(...selected.map((g) => g.x))}`)
})

test('cross-column drag includes left tail + right head in reading order', () => {
  const { glyphs, colTop } = twoColumnPage()
  const cols = computePageColumns(glyphs, PAGE2)
  // From middle of left column to middle of right column.
  const start = { x: 45, y: colTop + 3 * LINE_H + 1 }
  const end = { x: 500, y: colTop + 1 * LINE_H + 1 }
  const selected = clipGlyphsToSelection(glyphs, cols, start, end)
  assert.ok(selected.some((g) => colOf(g, cols) === 0), 'should include left glyphs')
  assert.ok(selected.some((g) => colOf(g, cols) === 1), 'should include right glyphs')
  // In reading order the last left-column glyph precedes the first right one.
  const lastLeft = selected.findLastIndex((g) => colOf(g, cols) === 0)
  const firstRight = selected.findIndex((g) => colOf(g, cols) === 1)
  assert.ok(lastLeft < firstRight, 'left column must come before right in reading order')
})

// --- single column still works ----------------------------------------------

test('single-column vertical drag selects the spanned lines', () => {
  const { glyphs } = singleColumnPage()
  const cols = computePageColumns(glyphs, PAGE)
  const start = { x: 45, y: 41 }
  const end = { x: 400, y: 40 + 5 * LINE_H + 1 }
  const selected = clipGlyphsToSelection(glyphs, cols, start, end)
  const text = glyphsToText(selected)
  assert.ok(text.length > 0, text)
  assert.ok(text.split('\n').length >= 5, `expected several lines, got: ${text.split('\n').length}`)
})

// --- full-width abstract selectable -----------------------------------------

test('selecting the abstract band grabs the full-width line', () => {
  const { glyphs } = twoColumnPage({ withAbstract: true })
  const cols = computePageColumns(glyphs, PAGE2)
  const selected = clipGlyphsToSelection(glyphs, cols, { x: 45, y: 41 }, { x: 660, y: 52 })
  // Abstract spans the full width: selection should include glyphs on both
  // sides of the gutter midline.
  assert.ok(selected.some((g) => g.x + g.width / 2 < GUTTER_MID), 'left half of abstract')
  assert.ok(selected.some((g) => g.x + g.width / 2 > GUTTER_MID), 'right half of abstract')
})

// --- geometry primitives ----------------------------------------------------

test('glyphAtPoint returns containing glyph, then nearest within tolerance', () => {
  const glyphs = [word('a', 10, 10), word('b', 30, 10)]
  assert.equal(glyphAtPoint(glyphs, { x: 12, y: 16 }), 0) // inside 'a'
  // Between the two, slightly closer to 'b'.
  const idx = glyphAtPoint(glyphs, { x: 31, y: 16 })
  assert.equal(idx, 1)
})

test('mergeGlyphsToLineRects produces one rect per contiguous line run', () => {
  const glyphs = line(['hello', 'world'], 40, 40)
  const rects = mergeGlyphsToLineRects(glyphs)
  assert.equal(rects.length, 1, JSON.stringify(rects))
  assert.ok(rects[0].width > 0 && rects[0].height > 0)
})

test('buildSelection returns text, rects and normalized bbox', () => {
  const { glyphs, colTop } = twoColumnPage()
  const cols = computePageColumns(glyphs, PAGE2)
  const sel = buildSelection({
    glyphs,
    columns: cols,
    start: { x: 45, y: colTop + 1 },
    end: { x: 200, y: colTop + 2 * LINE_H + 1 },
    pageSize: PAGE2,
  })
  assert.ok(!sel.isEmpty)
  assert.ok(sel.text.length > 0)
  assert.ok(sel.bboxList.length > 0)
  for (const box of sel.bboxList) {
    assert.equal(box.length, 4)
    assert.ok(box.every((v) => v >= 0 && v <= 1), JSON.stringify(box))
  }
})
