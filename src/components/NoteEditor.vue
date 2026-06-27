<script setup>
import { ref, computed, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { EditorState } from '@codemirror/state'
import { EditorView, keymap, lineNumbers, highlightActiveLine } from '@codemirror/view'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { markdown } from '@codemirror/lang-markdown'
import { syntaxHighlighting, defaultHighlightStyle } from '@codemirror/language'
import MarkdownText from './MarkdownText.vue'

// Knowledge-base pivot (P2): the Markdown editor for authored sources (notes,
// web clips, imported markdown/text). Body is plain Markdown — saving re-chunks
// it back through the same index pipeline so the note is immediately askable.
const props = defineProps({
  documentId: { type: String, required: true },
  ui: { type: Object, required: true },
  // Live index status of this document, mirrored from the parent's allDocs.
  indexStatus: { type: String, default: '' },
})

const emit = defineEmits(['saved', 'dirty-change'])

const title = ref('')
const body = ref('')
const contentType = ref('note')
const sourceUrl = ref('')
const loading = ref(false)
const saving = ref(false)
const error = ref('')
const savedFlash = ref(false)

const lastSavedTitle = ref('')
const lastSavedBody = ref('')

const editorHost = ref(null)
let view = null
let savedFlashTimer = null

const dirty = computed(
  () => title.value !== lastSavedTitle.value || body.value !== lastSavedBody.value,
)

const sourceLabel = computed(() => {
  const map = props.ui.contentTypeLabels || {}
  return map[contentType.value] || contentType.value.toUpperCase()
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
  } catch (err) {
    error.value = err?.message || String(err)
  } finally {
    loading.value = false
  }
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
  window.removeEventListener('keydown', onKeydown)
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

    <div class="note-editor-body">
      <div class="note-pane note-edit-pane">
        <div ref="editorHost" class="note-cm-host"></div>
      </div>
      <div class="note-pane note-preview-pane">
        <div class="note-preview-inner">
          <MarkdownText :text="body || ui.notePreviewEmpty || ''" />
        </div>
      </div>
    </div>
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

.note-edit-pane {
  border-right: 1px solid var(--line-soft);
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
</style>
