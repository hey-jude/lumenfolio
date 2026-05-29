// Glyph list -> plain text, in reading order.
//
// Input glyphs are assumed already clipped and sorted in reading order by
// clipGlyphsToSelection. We re-group into visual lines, join glyphs within a
// line (inserting spaces on real word gaps, merging soft hyphens at line
// breaks), and join lines with newlines.
//
// Pure / DOM-free for `node --test`.

import { medianNumber, rectBottom, rectRight } from './geometry.js'

/**
 * @param {Array<{x,y,width,height,text}>} glyphs - glyphs already in reading
 *   order (column-major) as produced by clipGlyphsToSelection. We must NOT
 *   globally re-sort here, or cross-column order would be lost; instead we group
 *   into lines by walking the sequence and breaking on a vertical jump.
 * @returns {string}
 */
export function glyphsToText(glyphs) {
  const usable = (glyphs || []).filter((g) => g && g.text)
  if (!usable.length) return ''

  const lines = groupSequentialLines(usable)
  const lineTexts = lines.map((line) => joinLineGlyphs(line))
  return normalizeText(mergeHyphenatedLines(lineTexts))
}

/**
 * Group an already-reading-ordered glyph sequence into visual lines without
 * re-sorting across columns. A new line starts when the next glyph's vertical
 * band no longer overlaps the current line's band (covers both row changes and
 * column jumps, since a column jump resets y upward).
 */
function groupSequentialLines(glyphs) {
  const lines = []
  let current = null
  let bandTop = 0
  let bandBottom = 0
  for (const g of glyphs) {
    const top = g.y
    const bottom = rectBottom(g)
    const overlap = current
      ? Math.min(bandBottom, bottom) - Math.max(bandTop, top)
      : -1
    const sameLine = current && overlap >= Math.min(g.height, bandBottom - bandTop) * 0.5
    if (!sameLine) {
      current = [g]
      lines.push(current)
      bandTop = top
      bandBottom = bottom
    } else {
      current.push(g)
      bandTop = Math.min(bandTop, top)
      bandBottom = Math.max(bandBottom, bottom)
    }
  }
  return lines
}

function joinLineGlyphs(line) {
  const sorted = [...line].sort((a, b) => a.x - b.x)
  const medianWidthPerChar = medianNumber(
    sorted.map((g) => g.width / Math.max(1, g.text ? g.text.length : 1)).filter(Number.isFinite),
  ) || 4
  const spaceGap = medianWidthPerChar * 0.6

  let text = ''
  let prevRight = null
  for (const g of sorted) {
    const piece = g.text
    if (prevRight === null) {
      text = piece
    } else {
      const gap = g.x - prevRight
      const needsSpace = gap > spaceGap && !/\s$/.test(text) && !/^\s/.test(piece)
      text += (needsSpace ? ' ' : '') + piece
    }
    prevRight = rectRight(g)
  }
  return text.trim()
}

/**
 * Merge a line ending in "word-" with the next line's leading lowercase word
 * (soft hyphenation), otherwise keep lines separate.
 */
function mergeHyphenatedLines(lineTexts) {
  const out = []
  for (const raw of lineTexts) {
    const line = raw
    const prev = out[out.length - 1]
    if (prev && /[A-Za-z]-$/.test(prev) && /^[a-z]/.test(line)) {
      out[out.length - 1] = prev.replace(/-$/, '') + line
    } else {
      out.push(line)
    }
  }
  return out.join('\n')
}

/** Collapse runs of spaces, trim each line, drop empty leading/trailing lines. */
export function normalizeText(text) {
  return String(text || '')
    .split('\n')
    .map((line) => line.replace(/[ \t]+/g, ' ').trim())
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}
