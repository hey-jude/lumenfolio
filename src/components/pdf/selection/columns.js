// Column-band precomputation for the custom selection kernel.
//
// The two-column selection bug exists because pdf.js gives glyphs in
// content-stream order, not reading order. The fix is to treat *column layout
// as a property of the page* (computed once from all glyphs on the page) and
// reconstruct reading order from it — independent of the selection.
//
// `computePageColumns` analyses every glyph on a page and returns:
//   { bands: [{ left, right, index }], fullWidth: [{ top, bottom }] }
// where `bands` are the body text columns (left-to-right) and `fullWidth` are
// page-spanning regions (abstract, title, full-width figures) that must not
// participate in column splitting.
//
// Pure / DOM-free for `node --test`.

import { groupGlyphsIntoLines, medianNumber, rectRight } from './geometry.js'

const DEFAULTS = {
  // A line wider than this fraction of page width is treated as full-width
  // (abstract, title, spanning caption) and excluded from column detection.
  fullWidthRatio: 0.72,
  // Minimum horizontal gutter between two columns, as a fraction of page width.
  minGutterRatio: 0.035,
  // Gutter must also be at least this multiple of the median glyph width.
  minGutterGlyphFactor: 2.0,
  // A column must contain at least this many lines to be considered real
  // (guards against a stray glyph in the gutter spawning a fake column).
  minLinesPerColumn: 2,
}

/**
 * Compute column bands and full-width regions for a page.
 *
 * @param {Array<{x,y,width,height,text}>} glyphs - all glyphs on the page
 * @param {{width:number,height:number}} pageSize - page-local pixel size
 * @returns {{ bands: Array<{left:number,right:number,index:number}>,
 *             fullWidth: Array<{top:number,bottom:number}> }}
 */
export function computePageColumns(glyphs, pageSize, options = {}) {
  const opts = { ...DEFAULTS, ...options }
  const pageWidth = Math.max(1, pageSize?.width || 0)
  const usable = (glyphs || []).filter((g) => g && g.width > 0 && g.height > 0)
  if (usable.length < 4) {
    return { bands: singleBand(pageWidth), fullWidth: [] }
  }

  const lines = groupGlyphsIntoLines(usable)
  const fullWidth = []
  const bodyGlyphs = []

  for (const line of lines) {
    const left = Math.min(...line.map((g) => g.x))
    const right = Math.max(...line.map((g) => rectRight(g)))
    const top = Math.min(...line.map((g) => g.y))
    const bottom = Math.max(...line.map((g) => g.y + g.height))
    if (right - left >= pageWidth * opts.fullWidthRatio) {
      fullWidth.push({ top, bottom })
      continue
    }
    bodyGlyphs.push(...line)
  }

  if (bodyGlyphs.length < 4) {
    return { bands: singleBand(pageWidth), fullWidth }
  }

  const bands = detectColumnBands(bodyGlyphs, pageWidth, opts)
  return { bands: bands.length ? bands : singleBand(pageWidth), fullWidth }
}

function singleBand(pageWidth) {
  return [{ left: 0, right: pageWidth, index: 0 }]
}

/**
 * Detect vertical column bands via 1-D gap analysis on glyph x-spans.
 *
 * We sweep the glyphs left-to-right tracking covered x-intervals; a gap wider
 * than the gutter threshold starts a new band. Bands with too few lines are
 * merged back so a few stray gutter glyphs cannot fabricate a column.
 */
function detectColumnBands(glyphs, pageWidth, opts) {
  const gutter = Math.max(
    pageWidth * opts.minGutterRatio,
    (medianNumber(glyphs.map((g) => g.width / Math.max(1, g.text ? g.text.length : 1)).filter(Number.isFinite)) || 4)
      * opts.minGutterGlyphFactor,
  )

  // Build covered x-intervals by merging glyph spans with small gaps.
  const spans = glyphs
    .map((g) => ({ left: g.x, right: rectRight(g) }))
    .sort((a, b) => a.left - b.left)

  const clusters = []
  for (const span of spans) {
    const last = clusters[clusters.length - 1]
    if (!last || span.left - last.right > gutter) {
      clusters.push({ left: span.left, right: span.right, glyphs: [] })
    } else {
      last.right = Math.max(last.right, span.right)
    }
  }

  if (clusters.length <= 1) {
    return singleBand(pageWidth)
  }

  // Assign glyphs to clusters and drop clusters with too few distinct lines.
  for (const g of glyphs) {
    const center = g.x + g.width / 2
    const cluster = clusters.find((c) => center >= c.left && center <= c.right)
      || nearestCluster(clusters, center)
    if (cluster) cluster.glyphs.push(g)
  }

  const realClusters = clusters.filter((c) => {
    const lineCount = groupGlyphsIntoLines(c.glyphs).length
    return lineCount >= opts.minLinesPerColumn
  })

  if (realClusters.length <= 1) {
    return singleBand(pageWidth)
  }

  // Expand bands to meet halfway across each gutter so a point in the gutter
  // maps deterministically to one side.
  const ordered = realClusters.sort((a, b) => a.left - b.left)
  const bands = ordered.map((c) => ({ left: c.left, right: c.right }))
  for (let i = 0; i < bands.length; i += 1) {
    const prevRight = i === 0 ? 0 : bands[i - 1].right
    const nextLeft = i === bands.length - 1 ? pageWidth : bands[i + 1].left
    const midPrev = i === 0 ? 0 : (prevRight + bands[i].left) / 2
    const midNext = i === bands.length - 1 ? pageWidth : (bands[i].right + nextLeft) / 2
    bands[i] = { left: midPrev, right: midNext, index: i }
  }
  // Snap outer edges to the page.
  bands[0].left = 0
  bands[bands.length - 1].right = pageWidth
  return bands
}

function nearestCluster(clusters, center) {
  let best = null
  let bestDistance = Infinity
  for (const c of clusters) {
    const distance = center < c.left ? c.left - center : center > c.right ? center - c.right : 0
    if (distance < bestDistance) {
      bestDistance = distance
      best = c
    }
  }
  return best
}

/**
 * Map an x coordinate to a column band index. Returns 0 when there is a single
 * band; for a gutter point, returns the nearest band. -1 only if no bands.
 */
export function columnIndexAt(x, bands) {
  if (!bands || !bands.length) return -1
  for (const band of bands) {
    if (x >= band.left && x <= band.right) return band.index
  }
  // Outside all bands -> clamp to nearest by edge distance.
  let best = bands[0]
  let bestDistance = Infinity
  for (const band of bands) {
    const distance = x < band.left ? band.left - x : x - band.right
    if (distance < bestDistance) {
      bestDistance = distance
      best = band
    }
  }
  return best.index
}

/** True if a y range overlaps any full-width region. */
export function intersectsFullWidth(top, bottom, fullWidth) {
  return (fullWidth || []).some((region) => bottom >= region.top && top <= region.bottom)
}
