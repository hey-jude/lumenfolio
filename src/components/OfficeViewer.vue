<script setup>
import { ref, shallowRef, watch, onMounted, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'

// Knowledge-base pivot (P3): client-side Office preview. docx → docx-preview
// (Apache-2.0, high fidelity); xlsx → exceljs parsed into HTML tables; pptx has
// no clean OSS renderer yet (licensing), so it shows a text-indexed notice.
const props = defineProps({
  documentId: { type: String, required: true },
  contentType: { type: String, required: true },
  title: { type: String, default: '' },
  ui: { type: Object, required: true },
})

const loading = ref(false)
const error = ref('')
const docxHost = ref(null)
// xlsx: [{ name, rows: [[cell, …], …] }]
const sheets = shallowRef([])

async function fetchBytes() {
  const bytes = await invoke('read_document_bytes', { docId: props.documentId })
  const u8 = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)
  return u8.buffer.slice(u8.byteOffset, u8.byteOffset + u8.byteLength)
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

async function renderXlsx(buffer) {
  const mod = await import('exceljs')
  const ExcelJS = mod.default ?? mod
  const workbook = new ExcelJS.Workbook()
  await workbook.xlsx.load(buffer)
  const parsed = []
  workbook.eachSheet((worksheet) => {
    const rows = []
    worksheet.eachRow({ includeEmpty: false }, (row) => {
      const cells = []
      row.eachCell({ includeEmpty: true }, (cell) => {
        cells.push(formatCell(cell.value))
      })
      rows.push(cells)
    })
    parsed.push({ name: worksheet.name, rows })
  })
  sheets.value = parsed
}

function formatCell(value) {
  if (value == null) return ''
  if (typeof value === 'object') {
    // exceljs rich text / formula / hyperlink shapes.
    if (value.text != null) return String(value.text)
    if (value.result != null) return String(value.result)
    if (Array.isArray(value.richText)) return value.richText.map((part) => part.text).join('')
    if (value instanceof Date) return value.toLocaleDateString()
    return ''
  }
  return String(value)
}

async function load() {
  loading.value = true
  error.value = ''
  sheets.value = []
  try {
    if (props.contentType === 'pptx') {
      loading.value = false
      return
    }
    const buffer = await fetchBytes()
    if (props.contentType === 'docx') {
      // The docx host sits behind the `v-if="loading"` chain, so it is NOT in the
      // DOM while loading. Drop the flag first so the host mounts, then render into
      // it (renderDocx awaits nextTick before touching docxHost).
      loading.value = false
      await renderDocx(buffer)
    } else if (props.contentType === 'xlsx') {
      await renderXlsx(buffer)
    }
  } catch (err) {
    error.value = err?.message || String(err)
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(() => props.documentId, load)
</script>

<template>
  <div class="office-viewer">
    <p v-if="loading" class="office-status">{{ ui.loading || 'Loading…' }}</p>
    <p v-else-if="error" class="office-status office-error">{{ error }}</p>

    <!-- pptx: no fidelity renderer yet; the deck's text is still indexed/askable. -->
    <div v-else-if="contentType === 'pptx'" class="office-placeholder">
      <div class="office-placeholder-icon" aria-hidden="true">📊</div>
      <p class="office-placeholder-title">{{ title || ui.slidesPreviewUnavailable }}</p>
      <p class="office-placeholder-note">{{ ui.pptxPreviewNote }}</p>
    </div>

    <!-- docx: rendered by docx-preview into this host. -->
    <div v-else-if="contentType === 'docx'" class="office-docx-scroll">
      <div ref="docxHost" class="office-docx-host"></div>
    </div>

    <!-- xlsx: simple per-sheet HTML tables. -->
    <div v-else-if="contentType === 'xlsx'" class="office-xlsx-scroll">
      <section v-for="sheet in sheets" :key="sheet.name" class="office-sheet">
        <h3 class="office-sheet-name">{{ sheet.name }}</h3>
        <table class="office-table">
          <tbody>
            <tr v-for="(row, rIndex) in sheet.rows" :key="rIndex">
              <td v-for="(cell, cIndex) in row" :key="cIndex">{{ cell }}</td>
            </tr>
          </tbody>
        </table>
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
.office-xlsx-scroll {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 16px;
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

.office-table td {
  border: 1px solid var(--line-soft);
  padding: 4px 8px;
  white-space: nowrap;
  max-width: 320px;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
