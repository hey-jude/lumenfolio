<script setup>
import { ref, shallowRef, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'

// Knowledge-base pivot (P3): client-side Office preview. docx → docx-preview
// (Apache-2.0, high fidelity); xlsx → exceljs parsed into HTML tables; pptx has
// no clean OSS renderer yet (licensing), so it shows a text-indexed notice.
const props = defineProps({
  documentId: { type: String, required: true },
  contentType: { type: String, required: true },
  title: { type: String, default: '' },
  // The citation the user clicked in chat, if any. Office sources carry no PDF
  // geometry, so evidence is anchored by content instead — see applyCitation().
  citation: { type: Object, default: null },
  ui: { type: Object, required: true },
})

const loading = ref(false)
const error = ref('')
const docxHost = ref(null)
const pptxHost = ref(null)
// True when the deck could not be rendered; falls back to the "text is indexed,
// ask in chat" note rather than showing an empty pane.
const pptxFailed = ref(false)
let pptxViewer = null
// xlsx: [{ name, rows: [[cell, …], …] }]
const sheets = shallowRef([])

async function fetchBytes() {
  const bytes = await invoke('read_document_bytes', { docId: props.documentId })
  const u8 = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)
  return u8.buffer.slice(u8.byteOffset, u8.byteOffset + u8.byteLength)
}

// pptx renders through @aiden0z/pptx-renderer (Apache-2.0): OOXML → HTML/SVG DOM,
// checked against PowerPoint output by visual regression. Fidelity was the goal
// here, so we take the real layout engine rather than an outline approximation.
// Windowed + lazy so a 200-slide deck doesn't build every slide up front, and
// RECOMMENDED_ZIP_LIMITS guards against a zip bomb — these are user files.
async function renderPptx(buffer) {
  const { PptxViewer, RECOMMENDED_ZIP_LIMITS } = await import('@aiden0z/pptx-renderer')
  await nextTick()
  if (!pptxHost.value) return
  destroyPptxViewer()
  pptxViewer = await PptxViewer.open(buffer, pptxHost.value, {
    zipLimits: RECOMMENDED_ZIP_LIMITS,
    lazySlides: true,
    lazyMedia: true,
    listOptions: { windowed: true, initialSlides: 4, batchSize: 4 },
    // PowerPoint stores SmartArt / pasted vector art as EMF with an embedded PDF
    // preview. pdfjs-dist is already a dependency for the PDF reader, so wiring
    // it in costs nothing and recovers those slides instead of dropping them.
    pdfjs: {
      moduleUrl: new URL('pdfjs-dist/build/pdf.min.mjs', import.meta.url).toString(),
      workerUrl: new URL('pdfjs-dist/build/pdf.worker.min.mjs', import.meta.url).toString(),
    },
  })
}

function destroyPptxViewer() {
  // Releases blob URLs, observers and DOM — without this every reopen leaks the
  // deck's decoded media.
  try {
    pptxViewer?.destroy?.()
  } catch {
    // A half-initialized viewer can throw here; nothing useful to do.
  }
  pptxViewer = null
}

async function renderDocx(buffer) {
  const { renderAsync } = await import('docx-preview')
  await nextTick()
  if (!docxHost.value) return
  docxHost.value.innerHTML = ''
  await renderAsync(buffer, docxHost.value, undefined, {
    className: 'docx',
    inWrapper: true,
    ignoreWidth: false,
    breakPages: true,
  })
}

// A preview, not a spreadsheet app: cap the grid so a 50k-row sheet can't spend
// the UI thread building DOM. Anything beyond is reported under the table.
const MAX_PREVIEW_ROWS = 500
const MAX_PREVIEW_COLS = 60

// exceljs indexes rows/columns from 1. These mirror the A1 addresses the agent's
// read_sheet tool cites, so the user can match "B7" against what it says.
function columnLetter(index) {
  let n = index
  let label = ''
  while (n > 0) {
    label = String.fromCharCode(65 + ((n - 1) % 26)) + label
    n = Math.floor((n - 1) / 26)
  }
  return label
}

function columnIndex(letters) {
  let n = 0
  for (const char of letters) n = n * 26 + (char.charCodeAt(0) - 64)
  return n
}

// Merged ranges ("B2:D3") → the span to place on the top-left cell, plus every
// cell the merge swallows. The swallowed ones must NOT be emitted, or the row
// shifts right; skipping them is what makes multi-level headers line up.
function parseMerges(worksheet) {
  const spans = new Map()
  const covered = new Set()
  for (const range of worksheet.model?.merges ?? []) {
    const match = /^([A-Z]+)(\d+):([A-Z]+)(\d+)$/.exec(String(range).toUpperCase())
    if (!match) continue
    const top = Number(match[2])
    const bottom = Number(match[4])
    const left = columnIndex(match[1])
    const right = columnIndex(match[3])
    spans.set(`${top},${left}`, { rowspan: bottom - top + 1, colspan: right - left + 1 })
    for (let r = top; r <= bottom; r += 1) {
      for (let c = left; c <= right; c += 1) {
        if (r !== top || c !== left) covered.add(`${r},${c}`)
      }
    }
  }
  return { spans, covered }
}

async function renderXlsx(buffer) {
  const mod = await import('exceljs')
  const ExcelJS = mod.default ?? mod
  const workbook = new ExcelJS.Workbook()
  await workbook.xlsx.load(buffer)
  const parsed = []
  workbook.eachSheet((worksheet) => {
    const { spans, covered } = parseMerges(worksheet)
    const totalCols = Math.max(worksheet.columnCount || 0, 1)
    const totalRows = worksheet.rowCount || 0
    const columnCount = Math.min(totalCols, MAX_PREVIEW_COLS)
    const rowCount = Math.min(totalRows, MAX_PREVIEW_ROWS)
    const columns = Array.from({ length: columnCount }, (_, i) => columnLetter(i + 1))
    const rows = []
    // Walk the grid positionally instead of exceljs's eachRow/eachCell: those stop
    // at the last populated cell of each row, so every row got a different <td>
    // count and the columns drifted out of alignment.
    for (let r = 1; r <= rowCount; r += 1) {
      const row = worksheet.getRow(r)
      const cells = []
      for (let c = 1; c <= columnCount; c += 1) {
        if (covered.has(`${r},${c}`)) continue
        const cell = row.getCell(c)
        const span = spans.get(`${r},${c}`)
        cells.push({
          key: `${r},${c}`,
          text: formatCell(cell),
          // Clamp so a merge running past the preview cap can't overflow the grid.
          colspan: Math.min(span?.colspan ?? 1, columnCount - c + 1),
          rowspan: Math.min(span?.rowspan ?? 1, rowCount - r + 1),
          bold: Boolean(cell.font?.bold),
          align: cell.alignment?.horizontal || '',
        })
      }
      rows.push({ number: r, cells })
    }
    parsed.push({
      name: worksheet.name,
      columns,
      rows,
      hiddenRows: Math.max(totalRows - rowCount, 0),
      hiddenCols: Math.max(totalCols - columnCount, 0),
    })
  })
  sheets.value = parsed
}

function formatCell(cell) {
  const value = cell?.value
  if (value == null) return ''
  if (value instanceof Date) return value.toISOString().slice(0, 10)
  if (typeof value === 'object') {
    if (Array.isArray(value.richText)) return value.richText.map((part) => part.text).join('')
    // Formula cells carry their cached result; hyperlinks carry display text.
    if (value.result != null) return String(value.result)
    if (value.text != null) return String(value.text)
    return ''
  }
  // exceljs hands back a percentage as its raw fraction, so 0.85 rendered as
  // "0.85". Apply the format's own precision to show "85%".
  if (typeof value === 'number' && typeof cell.numFmt === 'string' && cell.numFmt.includes('%')) {
    const decimals = (cell.numFmt.split('.')[1] || '').replace(/[^0#]/g, '').length
    return `${(value * 100).toFixed(decimals)}%`
  }
  return String(value)
}

// ── Citation anchoring ──────────────────────────────────────────────────────
// A PDF citation carries page + bbox; an Office one carries neither (the block
// index writes page 0 and an empty bbox). So evidence is located by content:
//   * xlsx — the indexed row record is deliberately unlike the rendered cells
//     ("Region: West | Revenue: 3140" vs three columns), so text search could
//     never find it. Extraction prefixes each record with `Sheet!row`, which is
//     parsed back out here and matched against the rendered row.
//   * docx — the indexed block IS the paragraph text, so a normalized text match
//     against the rendered DOM is both simpler and sturdier than counting
//     paragraphs (headings, tables and lists desynchronize any ordinal).

// `${sheetIndex}:${rowNumber}` of the row to highlight, or '' for none.
const citedRow = ref('')

// Keep in sync with extract_xlsx in src-tauri/src/office.rs, which emits
// "<sheet>!<row> · <record>". Greedy up to the last '!' so a sheet name
// containing '!' still parses.
const SHEET_ROW_RE = /^(.*)!(\d+)\s+·\s/

function normalizeText(value) {
  return String(value || '').replace(/\s+/g, ' ').trim()
}

function applyXlsxCitation(quote) {
  citedRow.value = ''
  const match = SHEET_ROW_RE.exec(quote)
  if (!match) return
  const [, sheetName, rowText] = match
  const wanted = normalizeText(sheetName).toLowerCase()
  let index = sheets.value.findIndex((sheet) => normalizeText(sheet.name).toLowerCase() === wanted)
  // A single-sheet workbook stays useful even if the name drifted (renamed after
  // indexing); with several sheets, guessing the wrong one would be worse.
  if (index < 0 && sheets.value.length === 1) index = 0
  if (index < 0) return
  const key = `${index}:${Number(rowText)}`
  // Only claim a hit if that row is actually rendered (it may be past the cap).
  if (!sheets.value[index].rows.some((row) => `${index}:${row.number}` === key)) return
  citedRow.value = key
}

function applyDocxCitation(quote) {
  const host = docxHost.value
  if (!host) return
  host.querySelectorAll('.office-cite-hit').forEach((el) => el.classList.remove('office-cite-hit'))
  // Long quotes are truncated head-first with a trailing "..." (truncate_chars in
  // the RAG layer); that marker is not in the rendered text, so drop it or the
  // prefix match below can never succeed.
  const needle = normalizeText(quote).replace(/(\.{3}|…)$/, '').trim()
  if (needle.length < 4) return
  const nodes = Array.from(host.querySelectorAll('p, li, td, th, h1, h2, h3, h4, h5, h6'))
  const scored = nodes
    .map((el) => ({ el, text: normalizeText(el.textContent) }))
    .filter((entry) => entry.text.length > 0)
  const hit =
    scored.find((entry) => entry.text === needle)
    // The quote may be truncated by the retrieval quote cap, so a prefix counts.
    || scored.find((entry) => entry.text.includes(needle))
    // Or the block may have merged runs the renderer splits across elements.
    || scored.find((entry) => entry.text.length > 24 && needle.includes(entry.text))
  if (!hit) return
  hit.el.classList.add('office-cite-hit')
  hit.el.scrollIntoView({ block: 'center', behavior: 'smooth' })
}

async function applyCitation() {
  const quote = props.citation?.quote
  if (!quote || props.citation?.documentId !== props.documentId) {
    citedRow.value = ''
    return
  }
  if (props.contentType === 'xlsx') {
    applyXlsxCitation(quote)
    await nextTick()
    const el = document.querySelector(`[data-cited-row="${citedRow.value}"]`)
    el?.scrollIntoView({ block: 'center', behavior: 'smooth' })
  } else if (props.contentType === 'docx') {
    applyDocxCitation(quote)
  }
}

async function load() {
  loading.value = true
  error.value = ''
  sheets.value = []
  pptxFailed.value = false
  destroyPptxViewer()
  try {
    const buffer = await fetchBytes()
    if (props.contentType === 'pptx') {
      // Same ordering reason as docx: the host is behind the `v-if="loading"`
      // chain, so drop the flag first to mount it, then render into it.
      loading.value = false
      try {
        await renderPptx(buffer)
      } catch (err) {
        // A deck we can't lay out is still fully indexed and askable — degrade to
        // the notice instead of an error page.
        console.warn('pptx render failed', err)
        pptxFailed.value = true
      }
      return
    }
    if (props.contentType === 'docx') {
      // The docx host sits behind the `v-if="loading"` chain, so it is NOT in the
      // DOM while loading. Drop the flag first so the host mounts, then render into
      // it (renderDocx awaits nextTick before touching docxHost).
      loading.value = false
      await renderDocx(buffer)
    } else if (props.contentType === 'xlsx') {
      await renderXlsx(buffer)
    }
    // A cross-document citation click switches tabs, so this component often
    // mounts with a citation already pending — anchor once the render exists.
    await applyCitation()
  } catch (err) {
    error.value = err?.message || String(err)
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(() => props.documentId, load)
// Clicking another citation in the same open document re-anchors without reload.
watch(() => props.citation, applyCitation)
onBeforeUnmount(destroyPptxViewer)
</script>

<template>
  <div class="office-viewer">
    <p v-if="loading" class="office-status">{{ ui.loading || 'Loading…' }}</p>
    <p v-else-if="error" class="office-status office-error">{{ error }}</p>

    <!-- pptx: rendered to HTML/SVG by pptx-renderer; the notice is the fallback
         for a deck it cannot lay out (still indexed and askable). -->
    <div v-else-if="contentType === 'pptx' && pptxFailed" class="office-placeholder">
      <div class="office-placeholder-icon" aria-hidden="true">📊</div>
      <p class="office-placeholder-title">{{ title || ui.slidesPreviewUnavailable }}</p>
      <p class="office-placeholder-note">{{ ui.pptxPreviewNote }}</p>
    </div>

    <div v-else-if="contentType === 'pptx'" class="office-pptx-scroll">
      <div ref="pptxHost" class="office-pptx-host"></div>
    </div>

    <!-- docx: rendered by docx-preview into this host. -->
    <div v-else-if="contentType === 'docx'" class="office-docx-scroll">
      <div ref="docxHost" class="office-docx-host"></div>
    </div>

    <!-- xlsx: per-sheet grid with A1 row/column headers and honored merges. -->
    <div v-else-if="contentType === 'xlsx'" class="office-xlsx-scroll">
      <section v-for="(sheet, sheetIndex) in sheets" :key="sheet.name" class="office-sheet">
        <h3 class="office-sheet-name">{{ sheet.name }}</h3>
        <table class="office-table">
          <thead>
            <tr>
              <th class="office-corner"></th>
              <th v-for="col in sheet.columns" :key="col" class="office-colhead">{{ col }}</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="row in sheet.rows"
              :key="row.number"
              :data-cited-row="`${sheetIndex}:${row.number}`"
              :class="{ 'is-cited': citedRow === `${sheetIndex}:${row.number}` }"
            >
              <th class="office-rowhead">{{ row.number }}</th>
              <td
                v-for="cell in row.cells"
                :key="cell.key"
                :colspan="cell.colspan"
                :rowspan="cell.rowspan"
                :class="{ 'is-bold': cell.bold }"
                :style="cell.align ? { textAlign: cell.align } : null"
              >{{ cell.text }}</td>
            </tr>
          </tbody>
        </table>
        <p v-if="sheet.hiddenRows || sheet.hiddenCols" class="office-sheet-note">
          {{ ui.sheetPreviewTruncated || 'Preview truncated' }}
          <template v-if="sheet.hiddenRows">· +{{ sheet.hiddenRows }} rows</template>
          <template v-if="sheet.hiddenCols">· +{{ sheet.hiddenCols }} cols</template>
        </p>
      </section>
    </div>
  </div>
</template>

<style scoped>
.office-viewer {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-base, var(--bg-panel));
}

.office-status {
  padding: 24px;
  color: var(--text-muted, var(--text-secondary));
  font-size: 14px;
}

.office-error {
  color: var(--danger, #e06a6a);
}

.office-placeholder {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--text-muted, var(--text-secondary));
  padding: 24px;
  text-align: center;
}

.office-placeholder-icon {
  font-size: 40px;
}

.office-placeholder-title {
  font-size: 16px;
  color: var(--text-primary);
  margin: 0;
}

.office-placeholder-note {
  font-size: 13px;
  margin: 0;
  max-width: 420px;
}

.office-docx-scroll,
.office-xlsx-scroll,
.office-pptx-scroll {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 16px;
}

/* Slides carry their own (usually light) backgrounds; centre them on the app's
   surface and let each scale down to the pane instead of forcing a sideways
   scroll on a narrow window. */
.office-pptx-host {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
}

.office-pptx-host :deep(svg),
.office-pptx-host :deep(img) {
  max-width: 100%;
}

/* docx-preview emits its own light-themed page; keep it on a neutral surface. */
.office-docx-host {
  display: flex;
  flex-direction: column;
  align-items: center;
}

.office-docx-host :deep(.docx-wrapper) {
  background: transparent;
  padding: 0;
}

.office-sheet {
  margin-bottom: 24px;
}

.office-sheet-name {
  font-size: 13px;
  color: var(--text-muted, var(--text-secondary));
  margin: 0 0 8px;
}

.office-table {
  border-collapse: collapse;
  font-size: 13px;
  color: var(--text-primary);
}

.office-table th,
.office-table td {
  border: 1px solid var(--line-soft);
  padding: 4px 8px;
  white-space: nowrap;
  max-width: 320px;
  overflow: hidden;
  text-overflow: ellipsis;
  vertical-align: middle;
}

/* Row numbers / column letters mirror the A1 addresses read_sheet reports, so a
   cell the agent cites ("B7") can be found by eye. They stay pinned while the
   grid scrolls. */
.office-corner,
.office-colhead,
.office-rowhead {
  position: sticky;
  background: var(--bg-panel, #1f1f24);
  color: var(--text-muted, var(--text-secondary));
  font-size: 11px;
  font-weight: var(--w-strong);
  text-align: center;
}

.office-corner,
.office-colhead {
  top: 0;
}

.office-corner,
.office-rowhead {
  left: 0;
}

.office-corner {
  z-index: 3;
}

.office-colhead {
  z-index: 2;
}

.office-rowhead {
  z-index: 1;
}

/* Deliberately a raw 700, not --w-strong: this class mirrors the workbook's own
   bold formatting, so it reports the document rather than styling the app.
   Toning it down to the UI weight would misrepresent the file. */
.office-table .is-bold {
  font-weight: 700;
  color: var(--text-primary);
}

.office-sheet-note {
  margin: 6px 0 0;
  font-size: 11px;
  color: var(--text-muted, var(--text-secondary));
}

/* Evidence anchoring: the row (xlsx) or paragraph (docx) a chat citation points
   at. Sustained tint so it stays findable after the scroll, plus a one-shot
   pulse to catch the eye on arrival. */
.office-table tr.is-cited td {
  background: rgba(240, 181, 74, 0.16);
  box-shadow: inset 0 0 0 1px rgba(240, 181, 74, 0.4);
}

.office-table tr.is-cited th {
  color: var(--text-primary);
}

.office-docx-host :deep(.office-cite-hit) {
  background: rgba(240, 181, 74, 0.28);
  border-radius: 3px;
  box-shadow: 0 0 0 3px rgba(240, 181, 74, 0.28);
}

.office-table tr.is-cited td,
.office-docx-host :deep(.office-cite-hit) {
  animation: office-cite-pulse 1.1s ease-out 1;
}

@keyframes office-cite-pulse {
  0% {
    background: rgba(240, 181, 74, 0.55);
  }
}

@media (prefers-reduced-motion: reduce) {
  .office-table tr.is-cited td,
  .office-docx-host :deep(.office-cite-hit) {
    animation: none;
  }
}
</style>
