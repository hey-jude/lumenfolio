// Reading-order selection clipping — the core of the two-column fix.
//
// Given the page's column bands (from columns.js) and a drag from `start` to
// `end` (page-local points), decide exactly which glyphs are selected, in
// reading order. This is what makes "drag in the left column" stay in the left
// column: glyphs in other columns are excluded by construction, not filtered
// after the fact.
//
// Rules (start column <= end column after normalization):
//   - same column   -> glyphs in that column between startY..endY
//   - across columns -> start column (startY -> bottom)
//                     + every column in between (whole)
//                     + end column (top -> endY)
//   - full-width regions overlapping the selected y-range are always included
//     (so a full-width abstract/title can be selected).
//   - first/last visual line is trimmed horizontally by start.x / end.x so the
//     selection begins/ends mid-line at the pointer, not at the column edge.
//
// Pure / DOM-free for `node --test`.

import { columnIndexAt, intersectsFullWidth } from './columns.js'
import { groupGlyphsIntoLines, rectCenterX, rectCenterY, rectRight } from './geometry.js'

/**
 * @param {Array} glyphs - all glyphs on the page (content-stream order ok)
 * @param {{bands,fullWidth}} columns - from computePageColumns
 * @param {{x,y}} start - drag start (page-local px)
 * @param {{x,y}} end - drag end (page-local px)
 * @returns {Array} selected glyphs, sorted in reading order
 */
export function clipGlyphsToSelection(glyphs, columns, start, end) {
  const usable = (glyphs || []).filter((g) => g && g.width > 0 && g.height > 0)
  if (!usable.length || !start || !end) return []

  const bands = columns?.bands?.length ? columns.bands : null
  const fullWidth = columns?.fullWidth || []

  // Normalize the drag into reading order (anchor -> focus). Reading order is
  // COLUMN-MAJOR: a point in an earlier column always precedes one in a later
  // column regardless of y. Only within the same column does y (then x) decide.
  // This is why a left-lower -> right-upper drag still reads left-then-right.
  const [from, to] = orderPoints(start, end, bands)
  const fromCol = bands ? columnIndexAt(from.x, bands) : 0
  const toCol = bands ? columnIndexAt(to.x, bands) : 0

  const selected = []
  for (const g of usable) {
    const gy = rectCenterY(g)

    // Full-width regions (abstract/title): include strictly by y-range,
    // regardless of which column the glyph's x falls into.
    if (intersectsFullWidth(g.y, g.y + g.height, fullWidth)) {
      if (gy >= from.y - lineTolerance(g) && gy <= to.y + lineTolerance(g)) {
        selected.push(g)
      }
      continue
    }

    const col = bands ? columnIndexAt(rectCenterX(g), bands) : 0

    if (fromCol === toCol) {
      if (col !== fromCol) continue
      if (gy >= from.y - lineTolerance(g) && gy <= to.y + lineTolerance(g)) selected.push(g)
      continue
    }

    // Across columns.
    if (col < fromCol || col > toCol) continue
    if (col === fromCol) {
      if (gy >= from.y - lineTolerance(g)) selected.push(g)
    } else if (col === toCol) {
      if (gy <= to.y + lineTolerance(g)) selected.push(g)
    } else {
      selected.push(g) // whole middle column
    }
  }

  const trimmed = trimEdgeLines(selected, from, to, fromCol, toCol, bands)
  return sortReadingOrder(trimmed, bands)
}

/** Tolerance (px) for treating a glyph as within the start/end row. */
function lineTolerance(glyph) {
  return Math.max(2, glyph.height * 0.5)
}

function orderPoints(a, b, bands) {
  // Column-major reading order: earlier column first.
  if (bands) {
    const ca = columnIndexAt(a.x, bands)
    const cb = columnIndexAt(b.x, bands)
    if (ca !== cb) return ca < cb ? [a, b] : [b, a]
  }
  // Same column (or no bands): top first; near-same row -> left first.
  const sameRow = Math.abs(a.y - b.y) <= 8
  if (sameRow) {
    return a.x <= b.x ? [a, b] : [b, a]
  }
  return a.y <= b.y ? [a, b] : [b, a]
}

/**
 * Trim the first and last *visual lines* of the selection horizontally to the
 * pointer x, so a drag that starts/ends mid-line does not grab the whole line.
 * Only applies to the start line of the start column and the end line of the
 * end column.
 */
function trimEdgeLines(glyphs, from, to, fromCol, toCol, bands) {
  if (!glyphs.length) return glyphs
  const colOf = (g) => (bands ? columnIndexAt(rectCenterX(g), bands) : 0)

  // Identify the topmost line in the start column and bottommost in end column.
  const startColGlyphs = glyphs.filter((g) => colOf(g) === fromCol)
  const endColGlyphs = glyphs.filter((g) => colOf(g) === toCol)

  const startLine = topLine(startColGlyphs)
  const endLine = bottomLine(endColGlyphs)

  const startLineSet = new Set(startLine)
  const endLineSet = new Set(endLine)

  return glyphs.filter((g) => {
    // Start line: drop glyphs entirely left of the start x (begin mid-line).
    if (startLineSet.has(g) && sameRow(from, g) && rectRight(g) < from.x - 0.5) return false
    // End line: drop glyphs entirely right of the end x (end mid-line).
    if (endLineSet.has(g) && sameRow(to, g) && g.x > to.x + 0.5) return false
    return true
  })
}

function sameRow(point, glyph) {
  return point.y >= glyph.y - glyph.height && point.y <= glyph.y + glyph.height * 2
}

function topLine(glyphs) {
  if (!glyphs.length) return []
  const lines = groupGlyphsIntoLines(glyphs)
  return lines[0] || []
}

function bottomLine(glyphs) {
  if (!glyphs.length) return []
  const lines = groupGlyphsIntoLines(glyphs)
  return lines[lines.length - 1] || []
}

/**
 * Sort glyphs into reading order: by column (left-to-right), then by line
 * (top-to-bottom), then by x within a line. Full-width regions are interleaved
 * by their y position relative to columns is non-trivial; for v1 we order
 * full-width glyphs purely by (y, x) and place column glyphs after by column.
 * In practice a single selection rarely mixes full-width and multi-column, so
 * we sort the whole set by (column, y, x) with full-width treated as column -1
 * (reads first), which matches "abstract above columns".
 */
export function sortReadingOrder(glyphs, bands) {
  const colOf = (g) => (bands ? columnIndexAt(rectCenterX(g), bands) : 0)
  return [...glyphs].sort((a, b) => {
    const ca = colOf(a)
    const cb = colOf(b)
    if (ca !== cb) return ca - cb
    const dy = a.y - b.y
    if (Math.abs(dy) > Math.max(2, Math.min(a.height, b.height) * 0.5)) return dy
    return a.x - b.x
  })
}
