// Column-band precomputation for the custom selection kernel.
//
// The two-column selection bug exists because the browser's native selection
// follows DOM order, which is not visual reading order on multi-column pages.
// The fix is to treat *column layout as a property of the page*: detect the
// columns once from the page's glyphs, then reconstruct reading order from them.
//
// `computePageColumns` returns:
//   { bands: [{ left, right, index }], fullWidth: [{ top, bottom }] }
// `bands` are the body text columns (left-to-right); `fullWidth` are
// page-spanning regions (title, abstract, spanning caption) that are selected
// by y-range rather than by column.
//
// IMPORTANT — input must be CHARACTER-LEVEL glyphs. pdf.js text items are coarse
// runs with a single, often inflated width, so an item's x-span/center is not
// where its text sits. The caller splits each item's string evenly across its
// width (see splitItemsIntoCharGlyphs in PdfViewer.vue), giving per-char rects
// whose centers faithfully reflect the page layout — in particular the column
// gutter then contains no glyph centers and becomes detectable.
//
// Pure / DOM-free for `node --test`.

import { groupGlyphsIntoLines } from './geometry.js'

const DEFAULTS = {
  // Histogram bin width (px) for the glyph-center projection.
  bin: 6,
  // A gutter (valley) must be at least this wide, as a fraction of page width.
  minGutterRatio: 0.02,
  // A valley bin counts as "empty" when its density is <= this fraction of the
  // typical (median) occupied-bin density. Lenient enough that a few full-width
  // lines crossing the gutter don't mask it.
  valleyRatio: 0.3,
  // A detected column must own at least this many text rows to be real.
  minLinesPerColumn: 3,
}

/**
 * Compute column bands and full-width regions for a page.
 *
 * @param {Array<{x,y,width,height,text}>} glyphs - CHARACTER-level glyphs
 * @param {{width:number,height:number}} pageSize - page-local pixel size
 * @returns {{ bands: Array<{left,right,index}>, fullWidth: Array<{top,bottom}> }}
 */
export function computePageColumns(glyphs, pageSize, options = {}) {
  const opts = { ...DEFAULTS, ...options }
  const pageWidth = Math.max(1, pageSize?.width || 0)
  const usable = (glyphs || []).filter((g) => g && g.width > 0 && g.height > 0)
  if (usable.length < 8) {
    return { bands: singleBand(pageWidth), fullWidth: [] }
  }

  const bands = detectColumnBands(usable, pageWidth, opts)
  const fullWidth = bands.length > 1 ? detectFullWidthRegions(usable, bands) : []
  return { bands: bands.length ? bands : singleBand(pageWidth), fullWidth }
}

function singleBand(pageWidth) {
  return [{ left: 0, right: pageWidth, index: 0 }]
}

/**
 * Detect columns from a 1-D histogram of glyph CENTERS.
 *
 * Columns appear as dense bands separated by a low-density valley (the gutter).
 * We bin centers, find the occupied span, then split at sustained valleys whose
 * density is far below the typical column density. Each resulting column is
 * validated to own enough text rows.
 */
function detectColumnBands(glyphs, pageWidth, opts) {
  const bin = Math.max(2, opts.bin)
  const binCount = Math.ceil(pageWidth / bin) + 1
  const hist = new Array(binCount).fill(0)
  for (const g of glyphs) {
    const center = g.x + g.width / 2
    const idx = Math.max(0, Math.min(binCount - 1, Math.floor(center / bin)))
    hist[idx] += 1
  }

  let first = hist.findIndex((c) => c > 0)
  let last = -1
  for (let i = hist.length - 1; i >= 0; i -= 1) {
    if (hist[i] > 0) { last = i; break }
  }
  if (first < 0 || last <= first) return singleBand(pageWidth)

  const occupied = hist.filter((c) => c > 0).sort((a, b) => a - b)
  const medianDensity = occupied[occupied.length >> 1] || 1
  const valleyThreshold = medianDensity * opts.valleyRatio
  const minValleyBins = Math.max(1, Math.round((pageWidth * opts.minGutterRatio) / bin))

  // Find valleys: runs of bins at/below the valley threshold, wide enough.
  const cuts = [] // bin index at each valley center -> a column boundary
  let run = 0
  for (let i = first; i <= last + 1; i += 1) {
    const low = i > last || hist[i] <= valleyThreshold
    if (low) {
      run += 1
    } else {
      if (run >= minValleyBins && i - run > first) {
        cuts.push(i - 1 - Math.floor(run / 2)) // mid of the valley
      }
      run = 0
    }
  }

  if (!cuts.length) return singleBand(pageWidth)

  // Build candidate columns between cuts.
  const edges = [first - 0.5, ...cuts.map((c) => c + 0.5), last + 0.5]
  const candidates = []
  for (let i = 0; i < edges.length - 1; i += 1) {
    candidates.push({ left: edges[i] * bin, right: edges[i + 1] * bin })
  }

  // Validate each candidate owns enough rows (a row "belongs" to the column its
  // leftmost glyph center falls in). Rows are visual lines; a two-column row is
  // split across columns by per-glyph membership instead.
  const rows = groupGlyphsIntoLines(glyphs)
  const owned = candidates.map(() => 0)
  for (const row of rows) {
    const present = new Set()
    for (const g of row) {
      const cx = g.x + g.width / 2
      const ci = candidates.findIndex((c) => cx >= c.left && cx < c.right)
      if (ci >= 0) present.add(ci)
    }
    present.forEach((ci) => { owned[ci] += 1 })
  }

  let real = candidates.filter((_, i) => owned[i] >= opts.minLinesPerColumn)
  if (real.length < 2) return singleBand(pageWidth)

  // Drop sliver columns: a real text column is reasonably wide. Narrow bands are
  // gutter regions wrongly split by a full-width line's characters crossing the
  // gutter, not real columns. Keep only bands wider than a fraction of the
  // widest detected band.
  real.sort((a, b) => a.left - b.left)
  const widest = Math.max(...real.map((c) => c.right - c.left))
  real = real.filter((c) => c.right - c.left >= widest * 0.45)
  if (real.length < 2) return singleBand(pageWidth)

  // Expand to meet neighbours at midpoints so every x maps to exactly one band.
  const ordered = real.sort((a, b) => a.left - b.left)
  const bands = ordered.map((c, i) => ({
    left: i === 0 ? 0 : (ordered[i - 1].right + c.left) / 2,
    right: i === ordered.length - 1 ? pageWidth : (c.right + ordered[i + 1].left) / 2,
    index: i,
  }))
  bands[0].left = 0
  bands[bands.length - 1].right = pageWidth
  return bands
}

/**
 * A full-width region (title / abstract / spanning caption) is a visual line
 * whose glyph centers occupy more than one column AND continuously cover the
 * gutter (i.e. real running text across the page, not two separate column rows
 * that leave the gutter empty).
 */
function detectFullWidthRegions(glyphs, bands) {
  const lines = groupGlyphsIntoLines(glyphs)
  const regions = []
  for (const line of lines) {
    if (!lineIsFullWidth(line, bands)) continue
    const top = Math.min(...line.map((g) => g.y))
    const bottom = Math.max(...line.map((g) => g.y + g.height))
    const last = regions[regions.length - 1]
    if (last && top <= last.bottom + 2) {
      last.bottom = Math.max(last.bottom, bottom)
    } else {
      regions.push({ top, bottom })
    }
  }
  return regions
}

function lineIsFullWidth(line, bands) {
  if (bands.length < 2) return false
  // Distinguish a full-width running-text line (title / abstract) from a normal
  // two-column body row by the line's largest horizontal GAP:
  //  - a two-column row has a wide empty gap at the gutter (left text ends, gap,
  //    right text begins);
  //  - a full-width line tiles continuously, so its largest gap is just a word
  //    space.
  // If the line has glyphs spanning into more than one band but NO wide gap,
  // it is full-width.
  const sorted = [...line].map((g) => ({ l: g.x, r: g.x + g.width })).sort((a, b) => a.l - b.l)
  const lineLeft = sorted[0].l
  const lineRight = Math.max(...sorted.map((s) => s.r))
  // Must visually span across at least one band boundary.
  const boundary = bands.find((b, i) => i < bands.length - 1 && lineLeft < b.right && lineRight > b.right)?.right
  if (boundary == null) return false

  // The discriminator is whether the line has an empty gap covering the gutter
  // boundary. A two-column row's content stops before the boundary on the left
  // and resumes after it on the right, so the boundary sits inside an empty gap.
  // A full-width line has a glyph straddling/adjacent to the boundary.
  for (const s of sorted) {
    if (s.l <= boundary && s.r >= boundary) return true // a glyph covers the boundary
  }
  // No glyph covers the boundary: is the boundary inside a wide empty gap
  // (two columns) or just past the line's end? Find the gap containing it.
  let prevR = -Infinity
  for (const s of sorted) {
    if (boundary > prevR && boundary < s.l) {
      // boundary is in the empty gap (prevR, s.l) -> two columns, not full-width
      return false
    }
    prevR = Math.max(prevR, s.r)
  }
  return false
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
