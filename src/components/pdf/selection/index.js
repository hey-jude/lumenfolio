// Public entry point for the custom PDF text-selection kernel.
//
// Given a page's glyphs, its precomputed column layout, and a drag from `start`
// to `end` (page-local pixels), produce the selection: reading-order text, the
// per-line highlight rects (page-local px), and normalized bbox list (0..1) for
// the citation contract sent to the backend.
//
// This is the single API PdfViewer.vue calls. Everything here is pure /
// DOM-free and unit-tested under `node --test`.

import { computePageColumns, columnIndexAt, intersectsFullWidth } from './columns.js'
import { boundsOf, mergeGlyphsToLineRects } from './geometry.js'
import { clipGlyphsToSelection } from './reading-order.js'
import { glyphsToText } from './text.js'

export { computePageColumns, columnIndexAt, intersectsFullWidth }
export { clipGlyphsToSelection } from './reading-order.js'
export { glyphsToText } from './text.js'
export {
  glyphAtPoint,
  mergeGlyphsToLineRects,
  boundsOf,
  boundsOf as selectionBounds,
} from './geometry.js'

/**
 * Build a selection from a drag.
 *
 * @param {object} params
 * @param {Array} params.glyphs - all glyphs on the page (content-stream order ok)
 * @param {{bands,fullWidth}} params.columns - precomputed page column layout
 * @param {{x,y}} params.start - drag start, page-local px
 * @param {{x,y}} params.end - drag end, page-local px
 * @param {{width,height}} params.pageSize - page-local px size, for bbox normalization
 * @returns {{ glyphs:Array, text:string, rects:Array, bboxList:Array, isEmpty:boolean }}
 */
export function buildSelection({ glyphs, columns, start, end, pageSize }) {
  const clipped = clipGlyphsToSelection(glyphs, columns, start, end)
  if (!clipped.length) {
    return { glyphs: [], text: '', rects: [], bboxList: [], isEmpty: true }
  }
  const text = glyphsToText(clipped)
  const rects = mergeGlyphsToLineRects(clipped)
  const bboxList = rectsToBboxList(rects, pageSize)
  return {
    glyphs: clipped,
    text,
    rects,
    bboxList,
    isEmpty: !text || !rects.length,
  }
}

/**
 * Convert page-local rects to the normalized [x1,y1,x2,y2] (0..1) bbox contract
 * the backend expects. Values are clamped to [0,1]; degenerate rects dropped.
 */
export function rectsToBboxList(rects, pageSize) {
  const width = Math.max(1, pageSize?.width || 0)
  const height = Math.max(1, pageSize?.height || 0)
  const out = []
  for (const rect of rects || []) {
    const x1 = clamp01(rect.x / width)
    const y1 = clamp01(rect.y / height)
    const x2 = clamp01((rect.x + rect.width) / width)
    const y2 = clamp01((rect.y + rect.height) / height)
    if (x2 - x1 <= 0.0005 || y2 - y1 <= 0.0005) continue
    out.push([x1, y1, x2, y2])
  }
  return out
}

function clamp01(value) {
  if (!Number.isFinite(value)) return 0
  if (value < 0) return 0
  if (value > 1) return 1
  return value
}
