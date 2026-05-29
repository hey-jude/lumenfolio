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

  // Detect columns from glyph X-CENTERS, not x-spans. pdf.js glyph widths are
  // frequently overestimated, so x-spans bleed across the gutter and merge the
  // two columns into one; centers stay inside their own column and reveal the
  // gutter reliably.
  const bands = detectColumnBands(usable, pageWidth, opts)

  // With columns known, a "full-width" line is one whose glyphs straddle more
  // than one band (title / abstract / spanning caption). Detecting it this way
  // (rather than by raw line width) is immune to width inflation.
  const fullWidth = bands.length > 1 ? detectFullWidthRegions(usable, bands) : []

  return { bands: bands.length ? bands : singleBand(pageWidth), fullWidth }
}

function singleBand(pageWidth) {
  return [{ left: 0, right: pageWidth, index: 0 }]
}

/**
 * Detect vertical column bands from a 1-D histogram of glyph X-CENTERS.
 *
 * We bin centers across the page width, find the empty/low "valley" bins, and
 * split into columns at sustained valleys (the gutter). A valley must be at
 * least `gutter` wide. Columns with too few lines are discarded so a stray
 * gutter glyph cannot fabricate a column.
 */
function detectColumnBands(glyphs, pageWidth, opts) {
  const medianGlyphWidth = medianNumber(
    glyphs.map((g) => g.width / Math.max(1, g.text ? g.text.length : 1)).filter(Number.isFinite),
  ) || 4
  const gutter = Math.max(pageWidth * opts.minGutterRatio, medianGlyphWidth * opts.minGutterGlyphFactor)

  const bin = Math.max(4, Math.round(medianGlyphWidth))
  const binCount = Math.ceil(pageWidth / bin) + 1
  const hist = new Array(binCount).fill(0)
  for (const g of glyphs) {
    const center = g.x + g.width / 2
    const idx = Math.max(0, Math.min(binCount - 1, Math.floor(center / bin)))
    hist[idx] += 1
  }

  // Find the leftmost and rightmost occupied bins (ignore page margins).
  let firstOccupied = hist.findIndex((c) => c > 0)
  let lastOccupied = -1
  for (let i = hist.length - 1; i >= 0; i -= 1) {
    if (hist[i] > 0) { lastOccupied = i; break }
  }
  if (firstOccupied < 0 || lastOccupied <= firstOccupied) {
    return singleBand(pageWidth)
  }

  // Candidate gutters: runs of LOW-density center-bins wider than the gutter
  // width. "Low" is relative to the typical column density, not strictly zero —
  // a few full-width lines (title/abstract) crossing the gutter leave a small
  // residual count there that must not mask an otherwise-clear gutter.
  // Lenient "low-density" threshold to PROPOSE candidate gutters; the
  // row-consistency check below is the strict validator. Leniency matters
  // because a few full-width lines (title/abstract) crossing the gutter leave a
  // residual count there that a strict zero/quarter-median test would reject.
  const occupied = hist.filter((c) => c > 0).sort((a, b) => a - b)
  const medianDensity = occupied.length ? occupied[occupied.length >> 1] : 1
  const lowThreshold = Math.max(0, medianDensity * 0.5)
  const gutterBins = Math.max(1, Math.round(gutter / bin))
  const candidates = [] // { startBin, endBin } of each low-density run (a possible gutter)
  let lowRun = 0
  for (let i = firstOccupied; i <= lastOccupied + 1; i += 1) {
    const low = i > lastOccupied || hist[i] <= lowThreshold
    if (low) {
      lowRun += 1
    } else {
      if (lowRun >= gutterBins) {
        candidates.push({ startBin: i - lowRun, endBin: i - 1 })
      }
      lowRun = 0
    }
  }

  // A real column gutter is empty across MOST text rows; an inter-word gap is
  // empty on only a few. Keep only candidate gutters that are vacant on a large
  // fraction of rows. This is what separates a true two-column layout from a
  // page that merely has wide word spacing.
  const rows = groupGlyphsIntoLines(glyphs)
  const trueGutters = candidates.filter((cand) => {
    const left = cand.startBin * bin
    const right = (cand.endBin + 1) * bin
    const mid = (left + right) / 2
    let rowsCrossing = 0
    let rowsSpanning = 0 // rows with text on BOTH sides of this gutter
    for (const row of rows) {
      const hasLeft = row.some((g) => g.x + g.width / 2 < left)
      const hasRight = row.some((g) => g.x + g.width / 2 > right)
      if (!hasLeft || !hasRight) continue
      rowsSpanning += 1
      const crossing = row.some((g) => g.x < mid && rectRight(g) > mid)
      if (crossing) rowsCrossing += 1
    }
    if (rowsSpanning < opts.minLinesPerColumn) return false
    // Vacant (no glyph crosses the gutter midline) on >=70% of spanning rows.
    return rowsCrossing / rowsSpanning <= 0.3
  })

  if (!trueGutters.length) {
    return singleBand(pageWidth)
  }

  // Build columns between the validated gutters.
  const columns = []
  let runStart = firstOccupied
  for (const g of trueGutters.sort((a, b) => a.startBin - b.startBin)) {
    columns.push({ startBin: runStart, endBin: g.startBin - 1 })
    runStart = g.endBin + 1
  }
  columns.push({ startBin: runStart, endBin: lastOccupied })

  if (columns.length <= 1) {
    return singleBand(pageWidth)
  }

  // Convert bin ranges to px and attach glyphs (by center) to validate.
  const raw = columns.map((c) => ({
    left: c.startBin * bin,
    right: (c.endBin + 1) * bin,
    glyphs: [],
  }))
  for (const g of glyphs) {
    const center = g.x + g.width / 2
    const col = raw.find((c) => center >= c.left && center < c.right) || nearestColumn(raw, center)
    if (col) col.glyphs.push(g)
  }

  const real = raw.filter((c) => groupGlyphsIntoLines(c.glyphs).length >= opts.minLinesPerColumn)
  if (real.length <= 1) {
    return singleBand(pageWidth)
  }

  // Expand each band to meet its neighbour at the gutter midpoint, so a point
  // anywhere (including the gutter) maps deterministically to one column.
  const ordered = real.sort((a, b) => a.left - b.left)
  const bands = ordered.map((c) => ({ left: c.left, right: c.right }))
  for (let i = 0; i < bands.length; i += 1) {
    const left = i === 0 ? 0 : (ordered[i - 1].right + ordered[i].left) / 2
    const right = i === bands.length - 1 ? pageWidth : (ordered[i].right + ordered[i + 1].left) / 2
    bands[i] = { left, right, index: i }
  }
  bands[0].left = 0
  bands[bands.length - 1].right = pageWidth
  return bands
}

function nearestColumn(columns, center) {
  let best = null
  let bestDistance = Infinity
  for (const c of columns) {
    const distance = center < c.left ? c.left - center : center > c.right ? center - c.right : 0
    if (distance < bestDistance) {
      bestDistance = distance
      best = c
    }
  }
  return best
}

/**
 * A full-width region is a band of y where a single visual line's glyphs have
 * centers in more than one column (title / abstract / spanning caption).
 * Returns merged {top,bottom} y-ranges.
 */
function detectFullWidthRegions(glyphs, bands) {
  const lines = groupGlyphsIntoLines(glyphs)
  const regions = []
  for (const line of lines) {
    if (!lineCrossesGutter(line, bands)) continue
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

/**
 * A line is full-width only if its glyphs actually CONTINUE across a gutter —
 * i.e. some glyph spans the gutter midline, or glyphs sit just on both sides of
 * it with no real gap. Two separate column rows at the same y (left row + right
 * row) have centers on both sides but leave the gutter EMPTY, so they are not
 * full-width. This is what stops body rows from being misflagged (the original
 * two-column selection bug, in miniature).
 */
function lineCrossesGutter(line, bands) {
  for (let i = 0; i < bands.length - 1; i += 1) {
    const mid = (bands[i].right + bands[i + 1].left) / 2
    const hasLeft = line.some((g) => g.x + g.width / 2 < mid)
    const hasRight = line.some((g) => g.x + g.width / 2 > mid)
    if (!hasLeft || !hasRight) continue
    // Require an actual glyph covering the gutter midline (continuous text),
    // not two columns straddling an empty gutter.
    const covers = line.some((g) => g.x <= mid && rectRight(g) >= mid)
    if (covers) return true
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
