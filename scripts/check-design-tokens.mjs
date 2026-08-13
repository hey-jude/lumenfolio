#!/usr/bin/env node
/**
 * Guards the design token layer against drift.
 *
 * The app once carried 148 raw hex values, 497 raw rgba() calls, sixteen border
 * radii and four different reds — not because anyone chose that, but because
 * there was nothing shared to reach for, so every component invented its own.
 * Budgets here are ceilings on what survived the migration, not targets: they
 * only ever move down.
 *
 * Colors in <script> blocks are ignored on purpose. Those feed sigma, canvas and
 * PDF.js, where a CSS variable would not resolve — and an annotation's color
 * belongs to the saved document, not to the app's theme.
 *
 * Run: npm run check:design-tokens
 */
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'

const SRC = 'src'
const TOKENS = 'src/styles/tokens.css'

/** Files whose style block paints something other than app chrome. */
const EXEMPT_FILES = new Set([TOKENS])

/**
 * Per-file ceilings for raw color lines that are deliberately not tokenized.
 * Every one of these has a comment at the site explaining why. Raising a number
 * here means the explanation has to exist too.
 */
const RAW_COLOR_BUDGET = {
  // Paints the translated page as a sheet of paper: light on purpose, and must
  // not re-tint when the app theme moves. Plus two decorative gradients.
  'components/ReaderPane.vue': 23,
  // Decorative gradients (rail brand mark, drop-target sheen, progress bar).
  'components/WorkspaceSidebar.vue': 12,
  // SVG strokes handed to the graph renderer, not chrome.
  'components/KnowledgeMiniGraph.vue': 7,
  // Agent-process state gradients.
  'components/ChatPane.vue': 6,
  // The app-wide scrollbar — one documented source, see the comment there.
  'styles/main.css': 2,
  // The annotation swatch's two-tone ring, which must stay legible on whatever
  // color the user picked.
  'components/PdfAnnotationViewer.vue': 2,
  // The PDF page host is white because the page is paper.
  'components/PdfViewer.vue': 2,
  // Shimmer gradient on the update banner.
  'App.vue': 2,
  // Concept-category ink.
  'components/KnowledgePane.vue': 1,
}

/** font-weight values that are allowed to stay numeric, with the reason. */
const RAW_WEIGHT_BUDGET = {
  // Mirrors the workbook's own bold formatting: reports the document rather
  // than styling the app.
  'components/OfficeViewer.vue': 1,
}

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) walk(path, out)
    else if (/\.(vue|css)$/.test(entry)) out.push(path)
  }
  return out
}

/** The regions of a file that are CSS: the whole thing, or its style blocks. */
function styleRegions(path, source) {
  if (path.endsWith('.css')) return [source]
  const blocks = []
  const re = /<style\b[^>]*>([\s\S]*?)<\/style>/g
  let match
  while ((match = re.exec(source))) blocks.push(match[1])
  return blocks
}

const RAW_COLOR = /#[0-9a-fA-F]{3,8}\b|rgba?\(\s*\d/
const RAW_WEIGHT = /font-weight:\s*\d/
const RAW_RADIUS = /border-radius:[^;]*\d+px/
const RAW_DURATION = /transition:[^;]*\d+(?:ms|s)\b/
// The lookbehind is what keeps this from matching the `ease` inside var(--ease).
const BARE_EASE = /transition:[^;]*(?<!-)\bease\b/
const RAW_SHADOW = /box-shadow:[^;]*(?:#[0-9a-fA-F]{3,8}\b|rgba?\(\s*\d)/

const failures = []
const counted = {}

for (const path of walk(SRC)) {
  if (EXEMPT_FILES.has(path)) continue
  const rel = path.slice(SRC.length + 1)
  const source = readFileSync(path, 'utf8')

  let colorLines = 0
  let weightLines = 0

  for (const block of styleRegions(path, source)) {
    for (const [index, line] of block.split('\n').entries()) {
      const at = `${rel}`
      if (RAW_COLOR.test(line)) colorLines++
      if (RAW_WEIGHT.test(line)) weightLines++
      // These three have no exemptions at all: there is a token for every case.
      if (RAW_RADIUS.test(line)) {
        failures.push(`${at}: raw border-radius — use --r-xs/sm/md/lg/xl/pill\n    ${line.trim()}`)
      }
      if (RAW_DURATION.test(line)) {
        failures.push(`${at}: raw transition timing — use --dur-fast/base/slow\n    ${line.trim()}`)
      }
      if (BARE_EASE.test(line)) {
        failures.push(`${at}: bare \`ease\` in a transition — use var(--ease)\n    ${line.trim()}`)
      }
      void index
    }
  }

  if (colorLines) counted[rel] = colorLines

  const colorBudget = RAW_COLOR_BUDGET[rel] ?? 0
  if (colorLines > colorBudget) {
    failures.push(
      `${rel}: ${colorLines} raw color lines, budget ${colorBudget}. ` +
        `Use a token from ${TOKENS}, or document why this one cannot be themed ` +
        `and raise the budget with that reason.`,
    )
  }

  const weightBudget = RAW_WEIGHT_BUDGET[rel] ?? 0
  if (weightLines > weightBudget) {
    failures.push(
      `${rel}: ${weightLines} raw font-weight declarations, budget ${weightBudget}. ` +
        `Use --w-normal/medium/strong.`,
    )
  }
}

/**
 * Every var(--x) must resolve, unless the call site supplies a fallback. This is
 * the check that would have caught the note editor rendering its own background
 * as nothing: --bg-base was never defined, and only the fallback was holding it up.
 */
const defined = new Set(
  [...readFileSync(TOKENS, 'utf8').matchAll(/^\s+(--[a-z0-9-]+)\s*:/gm)].map((m) => m[1]),
)
// Set from JS as an inline style rather than declared in CSS.
defined.add('--annotation-color')
defined.add('--scale-factor') // pdf.js owns this one
for (const path of [...walk(SRC), ...walk(SRC).filter((p) => p.endsWith('.js'))]) {
  const source = readFileSync(path, 'utf8')
  const local = new Set([...source.matchAll(/(--[a-z0-9-]+)\s*:/g)].map((m) => m[1]))
  for (const match of source.matchAll(/var\((--[a-z0-9-]+)(\s*,)?/g)) {
    if (match[2] || defined.has(match[1]) || local.has(match[1])) continue
    failures.push(
      `${path.slice(SRC.length + 1)}: var(${match[1]}) is not defined anywhere and has no fallback.`,
    )
  }
}

if (failures.length) {
  console.error('Design token check failed:\n')
  for (const failure of failures) console.error(`  ${failure}`)
  console.error(`\n${failures.length} problem(s).`)
  process.exit(1)
}

const total = Object.values(counted).reduce((sum, n) => sum + n, 0)
console.log(`Design tokens OK — ${total} documented raw color lines, all within budget.`)
