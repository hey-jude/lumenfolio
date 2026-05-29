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

// Two-column page: left column x≈40, right column x≈340, gutter in the middle.
// Optional full-width abstract band at the top.
function twoColumnPage({ withAbstract = false } = {}) {
  const glyphs = []
  let y = 40
  if (withAbstract) {
    // Full-width line spanning almost the whole page width.
    glyphs.push(...line(['Abstract', 'this', 'paper', 'studies', 'agentic', 'retrieval', 'across', 'wide', 'span', 'columns', 'evident'], 40, y))
    y += LINE_H * 2
  }
  const colTop = y
  // Left column: 5 lines.
  for (let i = 0; i < 5; i += 1) {
    glyphs.push(...line(['LEFT', `l${i}a`, `l${i}b`, `l${i}c`], 40, colTop + i * LINE_H))
  }
  // Right column: 5 lines, starting x ~340 (clear gutter after left col which
  // ends near x ~40 + 4 words ~ 40+ (4+3+3+3)*7 + spaces ≈ 160).
  for (let i = 0; i < 5; i += 1) {
    glyphs.push(...line(['RIGHT', `r${i}a`, `r${i}b`, `r${i}c`], 340, colTop + i * LINE_H))
  }
  return { glyphs, colTop }
}

function singleColumnPage() {
  const glyphs = []
  for (let i = 0; i < 6; i += 1) {
    glyphs.push(...line(['the', 'quick', 'brown', 'fox', `n${i}`], 40, 40 + i * LINE_H))
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
  const cols = computePageColumns(glyphs, PAGE)
  assert.equal(cols.bands.length, 2, JSON.stringify(cols.bands))
  assert.ok(cols.bands[0].right <= cols.bands[1].left + 1, 'bands should not overlap')
})

test('full-width abstract is detected and excluded from column split', () => {
  const { glyphs } = twoColumnPage({ withAbstract: true })
  const cols = computePageColumns(glyphs, PAGE)
  assert.equal(cols.bands.length, 2, 'body still two columns')
  assert.ok(cols.fullWidth.length >= 1, 'abstract recorded as full-width region')
})

test('columnIndexAt maps x to the right band and clamps gutter points', () => {
  const { glyphs } = twoColumnPage()
  const cols = computePageColumns(glyphs, PAGE)
  assert.equal(columnIndexAt(60, cols.bands), 0)
  assert.equal(columnIndexAt(360, cols.bands), 1)
})

// --- the core bug: drag in left column must not leak into right -------------

test('dragging down the left column selects only left-column glyphs', () => {
  const { glyphs, colTop } = twoColumnPage()
  const cols = computePageColumns(glyphs, PAGE)
  const start = { x: 45, y: colTop + 1 }
  const end = { x: 120, y: colTop + 4 * LINE_H + 1 }
  const selected = clipGlyphsToSelection(glyphs, cols, start, end)
  assert.ok(selected.length > 0)
  assert.ok(selected.every((g) => g.text.startsWith('LEFT') || /^l\d/.test(g.text)),
    `leaked non-left glyphs: ${selected.map((g) => g.text).join(',')}`)
})

test('dragging down the right column selects only right-column glyphs', () => {
  const { glyphs, colTop } = twoColumnPage()
  const cols = computePageColumns(glyphs, PAGE)
  const start = { x: 345, y: colTop + 1 }
  const end = { x: 420, y: colTop + 4 * LINE_H + 1 }
  const selected = clipGlyphsToSelection(glyphs, cols, start, end)
  assert.ok(selected.length > 0)
  assert.ok(selected.every((g) => g.text.startsWith('RIGHT') || /^r\d/.test(g.text)),
    `leaked non-right glyphs: ${selected.map((g) => g.text).join(',')}`)
})

test('cross-column drag includes left tail + right head in reading order', () => {
  const { glyphs, colTop } = twoColumnPage()
  const cols = computePageColumns(glyphs, PAGE)
  // From middle of left column to middle of right column.
  const start = { x: 45, y: colTop + 3 * LINE_H + 1 }
  const end = { x: 420, y: colTop + 1 * LINE_H + 1 }
  const selected = clipGlyphsToSelection(glyphs, cols, start, end)
  const text = glyphsToText(selected)
  const firstLeft = text.indexOf('LEFT')
  const firstRight = text.indexOf('RIGHT')
  assert.ok(firstLeft >= 0 && firstRight >= 0, `text=${text}`)
  assert.ok(firstLeft < firstRight, 'left column should come before right in reading order')
})

// --- single column still works ----------------------------------------------

test('single-column vertical drag selects the spanned lines', () => {
  const { glyphs } = singleColumnPage()
  const cols = computePageColumns(glyphs, PAGE)
  const start = { x: 45, y: 41 }
  const end = { x: 200, y: 40 + 5 * LINE_H + 1 }
  const selected = clipGlyphsToSelection(glyphs, cols, start, end)
  const text = glyphsToText(selected)
  assert.ok(text.includes('quick'), text)
  assert.ok(text.split('\n').length >= 5, `expected several lines, got: ${text}`)
})

// --- full-width abstract selectable -----------------------------------------

test('selecting the abstract band grabs the full-width line', () => {
  const { glyphs } = twoColumnPage({ withAbstract: true })
  const cols = computePageColumns(glyphs, PAGE)
  const start = { x: 45, y: 41 }
  const end = { x: 560, y: 52 }
  const selected = clipGlyphsToSelection(glyphs, cols, start, end)
  const text = glyphsToText(selected)
  assert.ok(text.includes('Abstract'), text)
  assert.ok(text.includes('columns'), 'should span across the full width: ' + text)
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
  const cols = computePageColumns(glyphs, PAGE)
  const sel = buildSelection({
    glyphs,
    columns: cols,
    start: { x: 45, y: colTop + 1 },
    end: { x: 120, y: colTop + 2 * LINE_H + 1 },
    pageSize: PAGE,
  })
  assert.ok(!sel.isEmpty)
  assert.ok(sel.text.length > 0)
  assert.ok(sel.bboxList.length > 0)
  for (const box of sel.bboxList) {
    assert.equal(box.length, 4)
    assert.ok(box.every((v) => v >= 0 && v <= 1), JSON.stringify(box))
  }
})
