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
const titleInput = ref(null)
let view = null
let savedFlashTimer = null

// Live (Typora-style) editing via Milkdown's Crepe. It reads/writes the SAME
// `body` ref as the CodeMirror source editor, so save / dirty / autosave / wikilink
// extraction are all unchanged — body_md stays the single source of truth. Loaded
// lazily so the (large) editor bundle only arrives when the mode is actually used.
const liveHost = ref(null)
let crepe = null
// Guards the markdownUpdated listener: Crepe's own serializer may normalize the
// document while it boots, and recording that would mark a pristine note dirty.
let crepeReady = false
const crepeLoading = ref(false)
const crepeError = ref('')

// View mode: 'split' (editor + preview), 'edit' (editor only), 'preview' (preview
// only). Persisted so the user's choice sticks across notes/sessions. `splitRatio`
// is the editor's fraction of the body width in split mode (debounced writes keep
// dragging cheap).
// Live (Milkdown/Crepe) is the default editing experience — you write formatted
// prose, not raw syntax. The storage key is versioned: the old key's stored value
// ('split' for anyone who used a previous build) would otherwise pin existing users
// to the old default forever. Bumping it re-seeds everyone onto Live; whatever they
// pick next persists under the new key.
const viewMode = usePersistedRef('noteEditorViewModeV2', 'live')
const splitRatio = usePersistedRef('noteEditorSplitRatio', 0.5, { debounceMs: 150 })
const bodyHost = ref(null)
let dividerDragging = false

const editPaneStyle = computed(() => (
  viewMode.value === 'split'
    ? { flexBasis: `${(splitRatio.value * 100).toFixed(2)}%`, flexGrow: '0', flexShrink: '0' }
    : {}
))

// Tear down / stand up Crepe as the user enters and leaves live mode. Keeping only
// one editor alive at a time avoids two views racing to own `body`.
async function destroyCrepe() {
  crepeReady = false
  const instance = crepe
  crepe = null
  if (!instance) return
  try {
    await instance.destroy()
  } catch {
    /* editor already torn down */
  }
}

async function mountCrepe() {
  await destroyCrepe()
  await nextTick()
  if (!liveHost.value) return
  crepeLoading.value = true
  crepeError.value = ''
  try {
    const [{ Crepe }, { wikiLinkPlugins }] = await Promise.all([
      import('@milkdown/crepe'),
      import('../editor/wikiLink.js'),
      import('@milkdown/crepe/theme/common/style.css'),
      import('@milkdown/crepe/theme/frame-dark.css'),
      // Crepe's Latex feature renders with katex, which needs its stylesheet. The
      // preview pane happens to import it too, but live mode must not depend on that
      // component being mounted — import it here so math is styled on its own terms.
      import('katex/dist/katex.min.css'),
    ])
    // Bail out if the user left live mode (or the note closed) while loading.
    if (viewMode.value !== 'live' || !liveHost.value) return
    const instance = new Crepe({
      root: liveHost.value,
      defaultValue: body.value,
      featureConfigs: {
        placeholder: {
          text: props.ui.liveEditorPlaceholder || 'Write, or type / for commands…',
        },
      },
    })
    // Teach the pipeline about [[wikilinks]] BEFORE create(): Crepe builds its Editor
    // in the constructor and only wires it up on create(), so this is the seam where
    // extra schema/remark plugins can still be registered. Without this, remark would
    // escape the brackets to \[\[…]] and break wikilink extraction on the next save.
    instance.editor.use(wikiLinkPlugins)
    instance.on((listener) => {
      listener.markdownUpdated((_ctx, markdown) => {
        if (!crepeReady) return
        body.value = markdown
      })
    })
    await instance.create()
    crepe = instance
    crepeReady = true
  } catch (err) {
    crepeError.value = err?.message || String(err)
  } finally {
    crepeLoading.value = false
  }
}

async function setViewMode(mode) {
  const previous = viewMode.value
  viewMode.value = mode
  if (mode === 'live') {
    await mountCrepe()
    return
  }
  if (previous === 'live') await destroyCrepe()
  if (mode !== 'preview') {
    // The editor was display:none in preview mode; CodeMirror must re-measure
    // once it's visible again or the cursor/scroll geometry is stale.
    await nextTick()
    view?.requestMeasure()
    // Live mode may have rewritten `body`; push it into the CodeMirror buffer so
    // the source view shows what was actually authored.
    if (previous === 'live' && view && view.state.doc.toString() !== body.value) {
      setEditorDoc(body.value)
    }
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

// Autosave. Saving is a DB write plus a re-index enqueue, and the backend's job
// queue already de-dupes a pending re-index for the same document — but we still
// debounce rather than save per keystroke, so the indexer only wakes once the user
// actually pauses. Cmd+S (immediate) and the save-on-close in onBeforeUnmount both
// remain as the belt-and-braces paths.
const AUTOSAVE_DELAY_MS = 1000
let autosaveTimer = null

function queueAutosave() {
  if (autosaveTimer) clearTimeout(autosaveTimer)
  autosaveTimer = setTimeout(() => {
    autosaveTimer = null
    if (dirty.value && !saving.value) void save()
  }, AUTOSAVE_DELAY_MS)
}

watch([title, body], () => {
  // load() assigns title/body and then immediately re-baselines lastSaved*, so a
  // freshly-loaded note is never dirty here and won't self-save on open.
  if (loading.value || !dirty.value) return
  queueAutosave()
})

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
    // Crepe is seeded with `defaultValue` at construction, so it can only be built
    // once the body has actually arrived — not at mount time when it's still empty.
    if (viewMode.value === 'live') await mountCrepe()
    focusTitleIfNew()
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

// Live mode renders wikilinks as <span data-wiki-link data-target="…">. Handle the
// click here on the host rather than inside a ProseMirror plugin: the event bubbles
// out of the editor, and this reuses the exact same resolve/create path as preview.
function onLiveClick(event) {
  const chip = event.target.closest?.('[data-wiki-link]')
  if (!chip) return
  event.preventDefault()
  navigateWiki(chip.getAttribute('data-target') || '')
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

// Write any pending edits out NOW and wait for it. The chat agent reads this note
// from the database (read_note_source), so without a flush it could answer about a
// version up to one autosave-debounce stale — i.e. missing the sentence the user
// just typed and is asking about.
async function flushSave() {
  if (autosaveTimer) {
    clearTimeout(autosaveTimer)
    autosaveTimer = null
  }
  if (dirty.value && !saving.value) await save()
}

// Apply the agent's precise edits to the CURRENT buffer.
//
// This is the compare-and-swap step, and the reason precise edits matter beyond
// saving tokens: the proposal was built from the note as it was when the agent read
// it, and the user may have kept typing since. Each oldText arrives already resolved
// to the note's verbatim text, so a plain exact match is enough here — and if a
// target no longer matches uniquely we refuse the whole apply rather than clobber
// whatever the user wrote in the meantime. A whole-note rewrite cannot do this: it
// would silently overwrite those edits.
function applyEditsToText(source, edits) {
  let next = source
  for (const [index, edit] of edits.entries()) {
    const oldText = String(edit?.oldText ?? '')
    const newText = String(edit?.newText ?? '')
    if (!oldText) throw new Error(`Edit ${index + 1}: empty oldText`)
    const first = next.indexOf(oldText)
    if (first === -1) {
      throw new Error(`Edit ${index + 1}: the text this edit targets is no longer in the note.`)
    }
    if (next.indexOf(oldText, first + oldText.length) !== -1) {
      throw new Error(`Edit ${index + 1}: the text this edit targets now appears more than once.`)
    }
    // Deleting a whole line takes its newline, or a blank line is left behind.
    const end = !newText && next[first + oldText.length] === '\n'
      ? first + oldText.length + 1
      : first + oldText.length
    // Splice rather than String.replace: a newText containing `$&` or `$1` would be
    // expanded as a replacement pattern.
    next = next.slice(0, first) + newText + next.slice(end)
  }
  return next
}

/** Apply an agent proposal: precise edits when present, else a full rewrite. */
async function applyProposal(proposal) {
  const edits = Array.isArray(proposal?.edits) ? proposal.edits : null
  if (edits && edits.length) {
    // Build in memory first so a failure leaves the note untouched.
    const next = applyEditsToText(body.value, edits)
    await applyMarkdown(next)
    return
  }
  await applyMarkdown(proposal?.content ?? '')
}

// Apply an agent-proposed rewrite. This is the ONLY path by which anything other
// than the user changes the note, and it deliberately goes through the editor rather
// than the database: the editor is the single writer, so an apply can never race the
// autosave. In live mode it lands as a ProseMirror transaction (replaceAll), which
// keeps the undo stack — Cmd+Z reverts the agent's edit like any other change.
async function applyMarkdown(markdown) {
  const next = String(markdown ?? '')
  if (viewMode.value === 'live' && crepe) {
    try {
      const { replaceAll } = await import('@milkdown/kit/utils')
      crepe.editor.action(replaceAll(next))
      // markdownUpdated syncs `body` back from the editor.
    } catch {
      body.value = next
      setEditorDoc(next)
    }
  } else {
    body.value = next
    setEditorDoc(next)
  }
  await nextTick()
  await flushSave()
}

defineExpose({ flushSave, applyMarkdown, applyProposal })

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

/** Move the caret into the note body, whichever editor is currently mounted. */
function focusBody() {
  if (viewMode.value === 'live') {
    liveHost.value?.querySelector('.ProseMirror')?.focus()
    return
  }
  view?.focus()
}

// Enter in the title jumps to the body rather than doing nothing — the title is a
// single line, and this matches the title-then-write flow Obsidian uses.
function onTitleKeydown(event) {
  if (event.key !== 'Enter') return
  event.preventDefault()
  focusBody()
}

// A brand-new note lands as the backend's placeholder title with an empty body.
// Focus and select it so the first thing you type names the note — the title IS the
// document's name, so this is the "name it first" flow. Selecting (rather than
// clearing) keeps a sensible name if the user just starts writing in the body.
function focusTitleIfNew() {
  if (title.value !== 'Untitled' || body.value !== '') return
  const input = titleInput.value
  if (!input) return
  input.focus()
  input.select()
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
  if (autosaveTimer) clearTimeout(autosaveTimer)
  if (dirty.value && !saving.value) void save()
  void destroyCrepe()
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
        ref="titleInput"
        v-model="title"
        class="note-title-input"
        type="text"
        :placeholder="ui.noteTitlePlaceholder"
        :disabled="loading"
        @keydown="onTitleKeydown"
      />
      <div class="note-head-meta">
        <div class="note-view-toggle" role="group" :aria-label="ui.viewMode || 'View'">
          <button
            type="button"
            :class="{ active: viewMode === 'live' }"
            :title="ui.viewLive || 'Live'"
            @click="setViewMode('live')"
          >{{ ui.viewLive || 'Live' }}</button>
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
        <!-- Autosaved: no Save button. This is a status read-out, not a control —
             Cmd+S still forces an immediate save for anyone who reaches for it. -->
        <span v-if="saving" class="note-index-status">{{ ui.saving || 'Saving…' }}</span>
        <span v-else-if="indexStatus === 'indexing'" class="note-index-status">{{ ui.indexing || 'Indexing…' }}</span>
        <span v-else-if="savedFlash" class="note-index-status note-saved">{{ ui.savedAndIndexed || 'Saved' }}</span>
      </div>
    </header>

    <p v-if="error" class="note-error">{{ error }}</p>

    <div ref="bodyHost" class="note-editor-body" :class="`mode-${viewMode}`">
      <!-- Live (Typora-style) WYSIWYG. The host stays mounted (v-show) so it exists
           when Crepe is built; only one editor instance is ever alive, so Crepe and
           the CodeMirror view never fight over `body`. -->
      <div v-show="viewMode === 'live'" class="note-pane note-live-pane">
        <div ref="liveHost" class="note-crepe-host" @click="onLiveClick"></div>
        <p v-if="crepeLoading" class="note-live-status">{{ ui.liveEditorLoading || 'Loading editor…' }}</p>
        <p v-else-if="crepeError" class="note-live-status note-live-error">
          {{ ui.liveEditorFailed || 'Live editor failed to load; use Edit mode.' }}
        </p>
      </div>
      <div
        v-show="viewMode !== 'preview' && viewMode !== 'live'"
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
        v-show="viewMode !== 'edit' && viewMode !== 'live'"
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

/* Live (Crepe) pane. Crepe ships its own theme CSS; these rules only host it —
   full height, a readable centred column, and the app's own surface behind it. */
.note-live-pane {
  position: relative;
}

.note-crepe-host {
  height: 100%;
}

/* Theme alignment: Crepe exposes its whole palette as --crepe-* custom properties,
   so map them onto the app's tokens instead of fighting its stylesheet's specificity.
   The imported frame-dark theme sets these on .milkdown; we override them there. */
.note-crepe-host :deep(.milkdown) {
  height: 100%;
  background: transparent;

  --crepe-color-background: transparent;
  --crepe-color-on-background: var(--text-primary);
  --crepe-color-surface: var(--bg-panel);
  --crepe-color-surface-low: var(--bg-elevated);
  --crepe-color-on-surface: var(--text-primary);
  --crepe-color-on-surface-variant: var(--text-secondary);
  --crepe-color-outline: var(--line-soft);
  --crepe-color-primary: var(--accent);
  --crepe-color-secondary: var(--accent-soft);
  --crepe-color-on-secondary: var(--text-primary);
  --crepe-color-inverse: var(--bg-elevated);
  --crepe-color-on-inverse: var(--text-primary);
  --crepe-color-inline-code: #ff9d9d;
  --crepe-color-error: #ffb3b3;
  --crepe-color-hover: rgba(255, 255, 255, 0.05);
  --crepe-color-selected: var(--accent-soft);
  --crepe-color-inline-area: var(--bg-elevated);

  /* The stock theme ships Noto Serif/Noto Sans/Space Mono — fonts this app doesn't
     load, so headings would fall back to a different family than the rest of the UI. */
  --crepe-font-title: inherit;
  --crepe-font-default: inherit;
  --crepe-font-code: 'SF Mono', 'JetBrains Mono', Menlo, Monaco, 'Courier New', monospace;

  /* frame-dark uses white-tinted shadows, which glow oddly on this darker surface. */
  --crepe-shadow-1: 0 1px 2px rgba(0, 0, 0, 0.4);
  --crepe-shadow-2: 0 4px 14px rgba(0, 0, 0, 0.45);
}

.note-crepe-host :deep(.milkdown .ProseMirror) {
  max-width: 820px;
  margin: 0 auto;
  padding: 8px 16px 48px;
}

/* [[wikilink]] chips inside the live editor. Atomic nodes, so they read as one
   clickable unit rather than editable bracket syntax. */
.note-crepe-host :deep(.wiki-link) {
  padding: 1px 4px;
  border-radius: 5px;
  background: rgba(106, 169, 255, 0.12);
  color: var(--accent, #6aa9ff);
  cursor: pointer;
  white-space: nowrap;
}

.note-crepe-host :deep(.wiki-link:hover) {
  background: rgba(106, 169, 255, 0.22);
}

.note-crepe-host :deep(.wiki-link.ProseMirror-selectednode) {
  outline: 1px solid rgba(106, 169, 255, 0.6);
}

.note-live-status {
  position: absolute;
  top: 12px;
  left: 50%;
  transform: translateX(-50%);
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
}

.note-live-error {
  color: var(--text-danger, #ffb3b3);
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
