<script setup>
import { ref, computed, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { EditorState } from '@codemirror/state'
import { EditorView, keymap, lineNumbers, highlightActiveLine } from '@codemirror/view'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { markdown } from '@codemirror/lang-markdown'
import { syntaxHighlighting, defaultHighlightStyle } from '@codemirror/language'
import MarkdownText from './MarkdownText.vue'
import { usePersistedRef } from '../persistedState.js'

// Knowledge-base pivot (P2): the Markdown editor for authored sources (notes,
// web clips, imported markdown/text). Body is plain Markdown — saving re-chunks
// it back through the same index pipeline so the note is immediately askable.
const props = defineProps({
  documentId: { type: String, required: true },
  ui: { type: Object, required: true },
  // Live index status of this document, mirrored from the parent's allDocs.
  indexStatus: { type: String, default: '' },
})

const emit = defineEmits(['saved', 'dirty-change', 'open-doc', 'create-from-link'])

const title = ref('')
const body = ref('')
const contentType = ref('note')
const sourceUrl = ref('')
const loading = ref(false)
const saving = ref(false)
const error = ref('')
const savedFlash = ref(false)
// Wikilink graph: { outbound: [{title, documentId|null}], backlinks: [{documentId, title}] }
const links = ref({ outbound: [], backlinks: [] })

const lastSavedTitle = ref('')
const lastSavedBody = ref('')

const editorHost = ref(null)
let view = null
let savedFlashTimer = null

// View mode: 'split' (editor + preview), 'edit' (editor only), 'preview' (preview
// only). Persisted so the user's choice sticks across notes/sessions. `splitRatio`
// is the editor's fraction of the body width in split mode (debounced writes keep
// dragging cheap).
const viewMode = usePersistedRef('noteEditorViewMode', 'split')
const splitRatio = usePersistedRef('noteEditorSplitRatio', 0.5, { debounceMs: 150 })
const bodyHost = ref(null)
let dividerDragging = false

const editPaneStyle = computed(() => (
  viewMode.value === 'split'
    ? { flexBasis: `${(splitRatio.value * 100).toFixed(2)}%`, flexGrow: '0', flexShrink: '0' }
    : {}
))

async function setViewMode(mode) {
  viewMode.value = mode
  if (mode !== 'preview') {
    // The editor was display:none in preview mode; CodeMirror must re-measure
    // once it's visible again or the cursor/scroll geometry is stale.
    await nextTick()
    view?.requestMeasure()
  }
}

function startDividerDrag(event) {
  if (!bodyHost.value) return
  dividerDragging = true
  event.preventDefault()
  window.addEventListener('pointermove', onDividerDrag)
  window.addEventListener('pointerup', stopDividerDrag, { once: true })
}

function onDividerDrag(event) {
  if (!dividerDragging || !bodyHost.value) return
  const rect = bodyHost.value.getBoundingClientRect()
  if (rect.width <= 0) return
  const ratio = (event.clientX - rect.left) / rect.width
  splitRatio.value = Math.min(0.8, Math.max(0.2, ratio))
}

function stopDividerDrag() {
  dividerDragging = false
  window.removeEventListener('pointermove', onDividerDrag)
}

const dirty = computed(
  () => title.value !== lastSavedTitle.value || body.value !== lastSavedBody.value,
)

const sourceLabel = computed(() => {
  const map = props.ui.contentTypeLabels || {}
  return map[contentType.value] || contentType.value.toUpperCase()
})

// Render [[Title]] / [[Title|alias]] as fragment links the preview click handler
// intercepts. Fragment hrefs (#…) pass markdown-it's link validator (custom
// schemes would be stripped).
const previewBody = computed(() => {
  const raw = body.value || ''
  if (!raw) return props.ui.notePreviewEmpty || ''
  return raw.replace(/\[\[([^\]\n]+)\]\]/g, (match, inner) => {
    const parts = String(inner).split('|')
    const target = parts[0].trim()
    const label = (parts.length > 1 ? parts.slice(1).join('|') : parts[0]).trim() || target
    if (!target) return match
    return `[${label}](#wiki:${encodeURIComponent(target)})`
  })
})

watch(dirty, (value) => emit('dirty-change', value))

function buildEditor() {
  if (!editorHost.value || view) return
  const state = EditorState.create({
    doc: body.value,
    extensions: [
      lineNumbers(),
      history(),
      highlightActiveLine(),
      keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
      markdown(),
      syntaxHighlighting(defaultHighlightStyle),
      EditorView.lineWrapping,
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          body.value = update.state.doc.toString()
        }
      }),
      editorTheme,
    ],
  })
  view = new EditorView({ state, parent: editorHost.value })
}

function setEditorDoc(text) {
  if (!view) return
  view.dispatch({
    changes: { from: 0, to: view.state.doc.length, insert: text },
  })
}

async function load() {
  loading.value = true
  error.value = ''
  try {
    const data = await invoke('load_note_source', { documentId: props.documentId })
    title.value = data.title || ''
    body.value = data.bodyMd || ''
    contentType.value = data.contentType || 'note'
    sourceUrl.value = data.sourceUrl || ''
    lastSavedTitle.value = title.value
    lastSavedBody.value = body.value
    await nextTick()
    setEditorDoc(body.value)
    loadLinks()
  } catch (err) {
    error.value = err?.message || String(err)
  } finally {
    loading.value = false
  }
}

async function loadLinks() {
  try {
    const result = await invoke('load_note_links', { documentId: props.documentId })
    links.value = {
      outbound: Array.isArray(result?.outbound) ? result.outbound : [],
      backlinks: Array.isArray(result?.backlinks) ? result.backlinks : [],
    }
  } catch {
    links.value = { outbound: [], backlinks: [] }
  }
}

// Resolve a [[wikilink]] click: navigate to the matching document, or ask the
// parent to create a new note with that title when it doesn't exist yet.
function navigateWiki(rawTitle) {
  const title = String(rawTitle || '').trim()
  if (!title) return
  const hit = (links.value.outbound || []).find(
    (link) => link.title.toLowerCase() === title.toLowerCase(),
  )
  if (hit && hit.documentId) {
    emit('open-doc', hit.documentId)
  } else {
    emit('create-from-link', title)
  }
}

function onPreviewClick(event) {
  const anchor = event.target.closest('a')
  if (!anchor) return
  const href = anchor.getAttribute('href') || ''
  if (!href.startsWith('#wiki:')) return
  event.preventDefault()
  navigateWiki(decodeURIComponent(href.slice('#wiki:'.length)))
}

async function save() {
  if (saving.value || !dirty.value) return
  saving.value = true
  error.value = ''
  try {
    await invoke('update_note_source', {
      input: {
        documentId: props.documentId,
        title: title.value,
        bodyMd: body.value,
      },
    })
    lastSavedTitle.value = title.value
    lastSavedBody.value = body.value
    emit('saved', { documentId: props.documentId, title: title.value })
    flashSaved()
    loadLinks()
  } catch (err) {
    error.value = err?.message || String(err)
  } finally {
    saving.value = false
  }
}

function flashSaved() {
  savedFlash.value = true
  if (savedFlashTimer) clearTimeout(savedFlashTimer)
  savedFlashTimer = setTimeout(() => {
    savedFlash.value = false
  }, 1800)
}

function onKeydown(event) {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
    event.preventDefault()
    save()
  }
}

onMounted(() => {
  buildEditor()
  load()
  window.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  // Autosave unsaved edits before teardown. The parent keys this component by
  // document id, so switching sources destroys/recreates it; without this the
  // CodeMirror buffer (and the user's edits) would be silently lost. Fire-and-
  // forget — the IPC call completes even after the component is gone.
  if (dirty.value && !saving.value) void save()
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('pointermove', onDividerDrag)
  window.removeEventListener('pointerup', stopDividerDrag)
  if (savedFlashTimer) clearTimeout(savedFlashTimer)
  if (view) {
    view.destroy()
    view = null
  }
})

watch(
  () => props.documentId,
  () => {
    load()
  },
)

const editorTheme = EditorView.theme(
  {
    '&': {
      height: '100%',
      fontSize: '14px',
      color: 'var(--text-primary)',
      backgroundColor: 'transparent',
    },
    '.cm-content': {
      fontFamily:
        "'SF Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
      caretColor: 'var(--accent, #6aa9ff)',
      padding: '14px 0',
    },
    '.cm-gutters': {
      backgroundColor: 'transparent',
      color: 'var(--text-muted, var(--text-secondary))',
      border: 'none',
    },
    '.cm-activeLine': { backgroundColor: 'var(--bg-elevated, rgba(255,255,255,0.03))' },
    '.cm-activeLineGutter': { backgroundColor: 'transparent' },
    '&.cm-focused': { outline: 'none' },
    '.cm-cursor': { borderLeftColor: 'var(--accent, #6aa9ff)' },
    '.cm-scroller': { lineHeight: '1.6' },
  },
  { dark: true },
)
</script>

<template>
  <div class="note-editor">
    <header class="note-editor-head">
      <input
        v-model="title"
        class="note-title-input"
        type="text"
        :placeholder="ui.noteTitlePlaceholder"
        :disabled="loading"
      />
      <div class="note-head-meta">
        <div class="note-view-toggle" role="group" :aria-label="ui.viewMode || 'View'">
          <button
            type="button"
            :class="{ active: viewMode === 'edit' }"
            :title="ui.viewEdit || 'Editor'"
            @click="setViewMode('edit')"
          >{{ ui.viewEdit || 'Edit' }}</button>
          <button
            type="button"
            :class="{ active: viewMode === 'split' }"
            :title="ui.viewSplit || 'Split'"
            @click="setViewMode('split')"
          >{{ ui.viewSplit || 'Split' }}</button>
          <button
            type="button"
            :class="{ active: viewMode === 'preview' }"
            :title="ui.viewPreview || 'Preview'"
            @click="setViewMode('preview')"
          >{{ ui.viewPreview || 'Preview' }}</button>
        </div>
        <span class="note-kind-badge">{{ sourceLabel }}</span>
        <a
          v-if="sourceUrl"
          class="note-source-link"
          :href="sourceUrl"
          target="_blank"
          rel="noopener noreferrer"
          :title="sourceUrl"
        >{{ ui.sourceLink || 'Source' }}</a>
        <span v-if="indexStatus === 'indexing'" class="note-index-status">{{ ui.indexing || 'Indexing…' }}</span>
        <span v-else-if="savedFlash" class="note-index-status note-saved">{{ ui.savedAndIndexed || 'Saved' }}</span>
        <button
          type="button"
          class="note-save-btn"
          :disabled="!dirty || saving"
          @click="save"
        >{{ saving ? (ui.saving || 'Saving…') : (ui.save || 'Save') }}</button>
      </div>
    </header>

    <p v-if="error" class="note-error">{{ error }}</p>

    <div ref="bodyHost" class="note-editor-body" :class="`mode-${viewMode}`">
      <div
        v-show="viewMode !== 'preview'"
        class="note-pane note-edit-pane"
        :style="editPaneStyle"
      >
        <div ref="editorHost" class="note-cm-host"></div>
      </div>
      <div
        v-if="viewMode === 'split'"
        class="note-divider"
        role="separator"
        aria-orientation="vertical"
        :title="ui.dragToResize || ''"
        @pointerdown="startDividerDrag"
      ></div>
      <div
        v-show="viewMode !== 'edit'"
        class="note-pane note-preview-pane"
      >
        <div class="note-preview-inner" @click="onPreviewClick">
          <MarkdownText :text="previewBody" />
        </div>
      </div>
    </div>

    <footer
      v-if="links.backlinks.length || links.outbound.length"
      class="note-links-bar"
    >
      <div v-if="links.backlinks.length" class="note-links-group">
        <span class="note-links-title">{{ ui.backlinks }}</span>
        <button
          v-for="back in links.backlinks"
          :key="`b-${back.documentId}`"
          type="button"
          class="note-link-chip"
          @click="emit('open-doc', back.documentId)"
        >{{ back.title }}</button>
      </div>
      <div v-if="links.outbound.length" class="note-links-group">
        <span class="note-links-title">{{ ui.outboundLinks }}</span>
        <button
          v-for="(link, index) in links.outbound"
          :key="`o-${index}`"
          type="button"
          class="note-link-chip"
          :class="{ unresolved: !link.documentId }"
          :title="link.documentId ? '' : (ui.unresolvedLink || '')"
          @click="navigateWiki(link.title)"
        >{{ link.title }}</button>
      </div>
    </footer>
  </div>
</template>

<style scoped>
.note-editor {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-base, var(--bg-panel));
}

.note-editor-head {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 14px 20px;
  border-bottom: 1px solid var(--line-soft);
}

.note-title-input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: transparent;
  color: var(--text-primary);
  font-size: 18px;
  font-weight: 500;
}

.note-head-meta {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 0 0 auto;
}

.note-view-toggle {
  display: inline-flex;
  border: 1px solid var(--line-soft);
  border-radius: 7px;
  overflow: hidden;
}

.note-view-toggle button {
  border: none;
  background: transparent;
  color: var(--text-muted, var(--text-secondary));
  font-size: 12px;
  padding: 3px 10px;
  cursor: pointer;
  font-family: inherit;
  transition:
    background 0.12s ease,
    color 0.12s ease;
}

.note-view-toggle button + button {
  border-left: 1px solid var(--line-soft);
}

.note-view-toggle button:hover {
  color: var(--text-primary);
}

.note-view-toggle button.active {
  background: var(--accent-soft, rgba(106, 169, 255, 0.16));
  color: var(--accent-text, var(--accent, #6aa9ff));
}

.note-kind-badge {
  font-size: 11px;
  color: var(--text-muted, var(--text-secondary));
  border: 1px solid var(--line-soft);
  border-radius: 6px;
  padding: 2px 7px;
}

.note-source-link {
  font-size: 12px;
  color: var(--accent, #6aa9ff);
  text-decoration: none;
}

.note-index-status {
  font-size: 12px;
  color: var(--text-muted, var(--text-secondary));
}

.note-index-status.note-saved {
  color: var(--success, #4caf86);
}

.note-save-btn {
  border: none;
  border-radius: 8px;
  background: var(--accent, #6aa9ff);
  color: #fff;
  font-size: 13px;
  padding: 6px 14px;
  cursor: pointer;
}

.note-save-btn:disabled {
  opacity: 0.45;
  cursor: default;
}

.note-error {
  margin: 0;
  padding: 8px 20px;
  color: var(--danger, #e06a6a);
  font-size: 13px;
}

.note-editor-body {
  flex: 1;
  min-height: 0;
  display: flex;
}

.note-pane {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
}

/* Draggable separator between editor and preview (split mode only). The grab
   area is wide; a thin line sits in its centre and highlights on hover. */
.note-divider {
  flex: 0 0 9px;
  align-self: stretch;
  position: relative;
  cursor: col-resize;
  touch-action: none;
}

.note-divider::after {
  content: '';
  position: absolute;
  inset: 0 4px;
  background: var(--line-soft);
  transition: background 0.12s ease;
}

.note-divider:hover::after {
  background: rgba(106, 169, 255, 0.55);
}

.note-cm-host {
  height: 100%;
  padding: 0 12px;
}

.note-preview-pane {
  background: var(--bg-panel);
}

.note-preview-inner {
  padding: 18px 24px;
}

.note-links-bar {
  flex: 0 0 auto;
  border-top: 1px solid var(--line-soft);
  padding: 10px 20px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 22%;
  overflow-y: auto;
}

.note-links-group {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}

.note-links-title {
  font-size: 11px;
  color: var(--text-muted, var(--text-secondary));
  margin-right: 4px;
}

.note-link-chip {
  border: 1px solid var(--line-soft);
  background: var(--accent-soft, rgba(106, 169, 255, 0.12));
  color: var(--accent, #6aa9ff);
  border-radius: 999px;
  padding: 3px 10px;
  font-size: 12px;
  cursor: pointer;
}

.note-link-chip.unresolved {
  background: transparent;
  color: var(--text-muted, var(--text-secondary));
  border-style: dashed;
}
</style>
