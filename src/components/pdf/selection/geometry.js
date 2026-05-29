// Geometry primitives for the custom PDF text-selection kernel.
//
// All functions are pure and DOM-free so they can be unit-tested under
// `node --test`. A "glyph" here is one positioned text item from pdf.js
// `getTextContent()`, already mapped to page-local pixel coordinates:
//
//   { text: string, x: number, y: number, width: number, height: number }
//
// `x,y` is the top-left corner; `y` grows downward (screen convention).
//
// IMPORTANT: glyph arrays coming from pdf.js are in *content-stream* order,
// which is NOT guaranteed to match visual reading order (this is the root cause
// of the two-column selection bug). Nothing in this module may assume the input
// array is sorted — ordering is reconstructed explicitly from coordinates.

/** Clamp `value` into the inclusive range [min, max]. */
export function clamp(value, min, max) {
  if (value < min) return min
  if (value > max) return max
  return value
}

/** Median of a numeric list (0 for empty). Ignores nothing — caller filters. */
export function medianNumber(values) {
  if (!values.length) return 0
  const sorted = [...values].sort((left, right) => left - right)
  const middle = Math.floor(sorted.length / 2)
  return sorted.length % 2 === 0
    ? (sorted[middle - 1] + sorted[middle]) / 2
    : sorted[middle]
}

/** Right edge of a glyph/rect. */
export function rectRight(rect) {
  return rect.x + rect.width
}

/** Bottom edge of a glyph/rect. */
export function rectBottom(rect) {
  return rect.y + rect.height
}

/** Horizontal center of a glyph/rect. */
export function rectCenterX(rect) {
  return rect.x + rect.width / 2
}

/** Vertical center of a glyph/rect. */
export function rectCenterY(rect) {
  return rect.y + rect.height / 2
}

/** Overlapping area between two axis-aligned rects (0 if disjoint). */
export function intersectionArea(a, b) {
  const x1 = Math.max(a.x, b.x)
  const y1 = Math.max(a.y, b.y)
  const x2 = Math.min(rectRight(a), rectRight(b))
  const y2 = Math.min(rectBottom(a), rectBottom(b))
  return Math.max(0, x2 - x1) * Math.max(0, y2 - y1)
}

/**
 * Fraction of vertical overlap between two rects, relative to the shorter one.
 * 1 = one fully covers the other's height; 0 = no vertical overlap.
 * Used for same-line detection (EmbedPDF uses an 80% threshold for merging).
 */
export function verticalOverlapRatio(a, b) {
  const top = Math.max(a.y, b.y)
  const bottom = Math.min(rectBottom(a), rectBottom(b))
  const overlap = bottom - top
  if (overlap <= 0) return 0
  const minHeight = Math.max(1, Math.min(a.height, b.height))
  return overlap / minHeight
}

/** Distance from a scalar `value` to the inclusive range [left, right]. */
export function distanceToRange(value, left, right) {
  if (value < left) return left - value
  if (value > right) return value - right
  return 0
}

/** Axis-aligned bounding box of a list of rects ({x,y,width,height}); null if empty. */
export function boundsOf(rects) {
  if (!rects.length) return null
  let left = Infinity
  let top = Infinity
  let right = -Infinity
  let bottom = -Infinity
  for (const rect of rects) {
    left = Math.min(left, rect.x)
    top = Math.min(top, rect.y)
    right = Math.max(right, rectRight(rect))
    bottom = Math.max(bottom, rectBottom(rect))
  }
  return { x: left, y: top, width: Math.max(0, right - left), height: Math.max(0, bottom - top) }
}

/**
 * Hit-test a point against a list of glyphs, returning the index of the best
 * match, or -1 if none. Two-pass strategy adapted from EmbedPDF's `glyphAt`:
 *
 *   Pass 1 — return the first glyph whose box strictly contains the point.
 *   Pass 2 — expand each box by tolerance on all sides and pick the nearest by
 *            Manhattan distance to the glyph center.
 *
 * Tolerance defaults to ~1.5x the median glyph height, so clicks landing in the
 * inter-character / inter-line gaps still resolve to a sensible glyph.
 */
export function glyphAtPoint(glyphs, point, toleranceFactor = 1.5) {
  if (!glyphs.length) return -1

  // Pass 1: exact containment.
  for (let i = 0; i < glyphs.length; i += 1) {
    const g = glyphs[i]
    if (point.x >= g.x && point.x <= rectRight(g) && point.y >= g.y && point.y <= rectBottom(g)) {
      return i
    }
  }

  // Pass 2: nearest within tolerance.
  const medianHeight = medianNumber(glyphs.map((g) => g.height).filter(Number.isFinite)) || 10
  const tolerance = Math.max(1, medianHeight * toleranceFactor)
  let best = -1
  let bestDistance = Infinity
  for (let i = 0; i < glyphs.length; i += 1) {
    const g = glyphs[i]
    const withinX = point.x >= g.x - tolerance && point.x <= rectRight(g) + tolerance
    const withinY = point.y >= g.y - tolerance && point.y <= rectBottom(g) + tolerance
    if (!withinX || !withinY) continue
    const distance = Math.abs(point.x - rectCenterX(g)) + Math.abs(point.y - rectCenterY(g))
    if (distance < bestDistance) {
      bestDistance = distance
      best = i
    }
  }
  return best
}

/**
 * Merge a set of glyph rects into per-line selection rectangles.
 *
 * Glyphs are first grouped into visual lines by vertical overlap (>= 50% of the
 * shorter height), then each line's glyphs are merged left-to-right into runs,
 * breaking a run only when the horizontal gap exceeds `gapFactor` x the median
 * glyph width (so column gaps or large word gaps produce separate rects rather
 * than one rect spanning the gap).
 *
 * Returns rects sorted in reading order (top-to-bottom, left-to-right). Because
 * the caller has already clipped glyphs to a single column / reading slice, no
 * rect here can span across a column gutter.
 */
export function mergeGlyphsToLineRects(glyphs, { gapFactor = 2.5 } = {}) {
  const usable = glyphs.filter((g) => g && g.width > 0 && g.height > 0)
  if (!usable.length) return []

  const medianWidthPerGlyph = medianNumber(
    usable.map((g) => g.width / Math.max(1, g.text ? g.text.length : 1)).filter(Number.isFinite),
  ) || 4
  const gapThreshold = Math.max(1, medianWidthPerGlyph * gapFactor)

  const lines = groupGlyphsIntoLines(usable)
  const rects = []
  for (const line of lines) {
    const sorted = [...line].sort((a, b) => a.x - b.x)
    let run = null
    for (const g of sorted) {
      if (!run) {
        run = { x: g.x, y: g.y, right: rectRight(g), bottom: rectBottom(g) }
        continue
      }
      const gap = g.x - run.right
      if (gap > gapThreshold) {
        rects.push(runToRect(run))
        run = { x: g.x, y: g.y, right: rectRight(g), bottom: rectBottom(g) }
        continue
      }
      run.right = Math.max(run.right, rectRight(g))
      run.x = Math.min(run.x, g.x)
      run.y = Math.min(run.y, g.y)
      run.bottom = Math.max(run.bottom, rectBottom(g))
    }
    if (run) rects.push(runToRect(run))
  }
  return rects
}

function runToRect(run) {
  return {
    x: run.x,
    y: run.y,
    width: Math.max(0, run.right - run.x),
    height: Math.max(0, run.bottom - run.y),
  }
}

/**
 * Group glyphs into visual lines. Two glyphs share a line when their vertical
 * overlap is >= 50% of the shorter height. Input order is irrelevant; lines are
 * returned sorted top-to-bottom by their median y.
 */
export function groupGlyphsIntoLines(glyphs) {
  const sorted = [...glyphs].sort((a, b) => a.y - b.y || a.x - b.x)
  const lines = []
  for (const g of sorted) {
    const line = lines.find((entry) => verticalOverlapRatio(entry.sample, g) >= 0.5)
    if (line) {
      line.glyphs.push(g)
      // Keep the sample as the tallest glyph so short punctuation does not
      // shrink the line band.
      if (g.height > line.sample.height) line.sample = g
    } else {
      lines.push({ sample: g, glyphs: [g] })
    }
  }
  return lines
    .sort((a, b) => rectCenterY(a.sample) - rectCenterY(b.sample))
    .map((line) => line.glyphs)
}
