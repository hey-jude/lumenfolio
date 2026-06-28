<script setup>
import { computed, ref, onBeforeUnmount } from 'vue'
import lumenfolioLogo from '../assets/lumenfolio-logo-transparent.png'
import { startWindowDrag } from '../windowDrag'

const props = defineProps({
  roots: {
    type: Array,
    default: () => [],
  },
  selectedDocId: {
    type: String,
    required: true,
  },
  selectedDoc: {
    type: Object,
    default: null,
  },
  filter: {
    type: String,
    default: '',
  },
  scanStatus: {
    type: String,
    default: 'idle',
  },
  scanError: {
    type: String,
    default: '',
  },
  collapsed: {
    type: Boolean,
    default: false,
  },
  locale: {
    type: String,
    required: true,
  },
  ui: {
    type: Object,
    required: true,
  },
  dropActive: {
    type: Boolean,
    default: false,
  },
  dropTargetRootId: {
    type: String,
    default: '',
  },
  trendingActive: {
    type: Boolean,
    default: false,
  },
  trendingEnabled: {
    type: Boolean,
    default: true,
  },
  graphActive: {
    type: Boolean,
    default: false,
  },
  graphEnabled: {
    type: Boolean,
    default: true,
  },
})

const emit = defineEmits([
  'update:filter',
  'open-trending',
  'open-graph',
  'new-note',
  'select-doc',
  'add-folder',
  'import-files',
  'add-pdfs',
  'rescan',
  'reindex-doc',
  'delete-doc',
  'open-settings',
  'open-workspace',
  'delete-root',
  'toggle-root',
  'toggle-collapse',
  'workspace-drop',
  'set-drop-active',
])

const normalizedFilter = computed(() => String(props.filter || '').trim().toLowerCase())
const hasWorkspace = computed(() => props.roots.some((root) => Boolean(String(root.path || '').trim())))
const isScanning = computed(() => props.scanStatus === 'choosing' || props.scanStatus === 'scanning')
const allDocs = computed(() => props.roots.flatMap((root) => (
  (root.folders || []).flatMap((folder) => folder.docs || [])
)))
const visibleRailDocs = computed(() => props.roots.flatMap((root) => (
  root.collapsed ? [] : (root.folders || []).flatMap((folder) => folder.docs || [])
)))
const selectedRoot = computed(() => props.roots.find((root) => (
  (root.folders || []).some((folder) => (folder.docs || []).some((doc) => doc.id === props.selectedDocId))
)) || props.roots[0] || null)
const dragDepth = ref(0)

const isDropActive = computed(() => props.dropActive || dragDepth.value > 0)

function parseTextList(text) {
  return String(text || '')
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => {
      if (!line || line.startsWith('#')) return false
      if (line.startsWith('file://')) return true
      return /^[A-Za-z]:[\\/]|^\//.test(line)
    })
}

function hasFileEntries(payload) {
  const dataTransfer = payload
  if (!dataTransfer) return false
  if (dataTransfer.files && dataTransfer.files.length) return true

  const types = Array.from(dataTransfer.types || []).map((type) => String(type))
  if (types.includes('Files') || types.includes('application/x-moz-file')) return true
  if (types.includes('public.file-url')) return true
  if (types.includes('URL')) return true

  if (parseTextList(dataTransfer.getData?.('text/uri-list')).length > 0) return true
  if (parseTextList(dataTransfer.getData?.('text/plain')).length > 0) return true
  if (parseTextList(dataTransfer.getData?.('public.file-url')).length > 0) return true
  if (parseTextList(dataTransfer.getData?.('URL')).length > 0) return true
  if (parseTextList(dataTransfer.getData?.('application/x-moz-file')).length > 0) return true

  const items = Array.from(dataTransfer.items || [])
  return items.some((item) => item && item.kind === 'file')
}

function visibleDocs(docs) {
  if (!normalizedFilter.value) return docs
  return docs.filter((doc) => String(doc.title || '').toLowerCase().includes(normalizedFilter.value))
}

function emitDropActive(nextState) {
  emit('set-drop-active', Boolean(nextState))
}

function parseDroppedFilePaths(dataTransfer) {
  if (!dataTransfer) return []
  const files = Array.from(dataTransfer.files || [])
  const filePaths = files
    .map((file) => file?.path)
    .filter((path) => Boolean(path))
    .map((path) => String(path))

  if (filePaths.length) return filePaths

  const uriListPaths = parseTextList(dataTransfer.getData?.('text/uri-list'))
  if (uriListPaths.length) return uriListPaths

  const plainTextPaths = parseTextList(dataTransfer.getData?.('text/plain'))
  if (plainTextPaths.length) return plainTextPaths

  const publicFileUrlPaths = parseTextList(dataTransfer.getData?.('public.file-url'))
  if (publicFileUrlPaths.length) return publicFileUrlPaths

  const urlPaths = parseTextList(dataTransfer.getData?.('URL'))
  if (urlPaths.length) return urlPaths

  const mozillaUrlPaths = parseTextList(dataTransfer.getData?.('application/x-moz-file'))
  if (mozillaUrlPaths.length) return mozillaUrlPaths

  return []
}

function handleDragEnter(event) {
  if (!event?.dataTransfer) return
  event.preventDefault()
  if (!hasFileEntries(event.dataTransfer)) return
  dragDepth.value += 1
  emitDropActive(true)
}

function handleDragOver(event) {
  if (!event?.dataTransfer) return
  event.preventDefault()
  if (!hasFileEntries(event.dataTransfer)) return
  event.dataTransfer.dropEffect = 'copy'
  if (dragDepth.value <= 0) {
    dragDepth.value = 1
    emitDropActive(true)
  }
}

function handleDragLeave(event) {
  if (!event?.dataTransfer) return
  event.preventDefault()
  dragDepth.value = Math.max(0, dragDepth.value - 1)
  if (dragDepth.value === 0) emitDropActive(false)
}

function handleDrop(event) {
  if (!event?.dataTransfer) return
  event.preventDefault()
  const paths = parseDroppedFilePaths(event.dataTransfer)
  emitDropActive(false)
  dragDepth.value = 0
  if (!paths.length) return
  emit('workspace-drop', paths)
}

// Drag a chat-ready document onto the Chat composer to @-reference it. Uses a
// dedicated MIME so the composer can tell it apart from an image-file drop, and
// so it never collides with the sidebar's own file-drop (which carries files).
function handleDocDragStart(event, doc) {
  if (!doc?.chatReady || !event?.dataTransfer) return
  event.dataTransfer.setData('application/x-lumenfolio-doc-id', doc.id)
  event.dataTransfer.effectAllowed = 'move'
}

function localized(value) {
  if (!value || typeof value !== 'object') return value
  return value[props.locale] || value.en || Object.values(value)[0]
}

function statusLabel(status, doc = null) {
  if (status === 'indexed') return props.ui.statusIndexed
  if (status === 'indexing') {
    const percent = Number(doc?.indexProgress?.percent || 0)
    return percent > 0 && percent < 100
      ? `${props.ui.statusIndexing} ${percent}%`
      : props.ui.statusIndexing
  }
  if (status === 'stale') return props.ui.statusStale
  return status
}

function progressPercent(doc) {
  const percent = Number(doc?.indexProgress?.percent || 0)
  return Number.isFinite(percent) ? Math.max(0, Math.min(100, percent)) : 0
}

function treeLabel(doc) {
  return doc?.treeReady ? props.ui.treeReady : props.ui.treeMissing
}

function docStatusKind(doc) {
  const status = doc?.indexStatus
  if (status === 'indexed') return 'ready'
  if (status === 'stale') return 'failed'
  return 'processing'
}

function docStatusTitle(doc) {
  const kind = docStatusKind(doc)
  if (kind === 'ready') return props.ui.docStatusReady
  if (kind === 'failed') return props.ui.docStatusFailed
  return props.ui.docStatusProcessing
}

function compactDocLabel(doc) {
  const name = String(doc?.shortTitle || doc?.title || 'PDF').replace(/\.pdf$/i, '')
  const compact = name
    .replace(/[^A-Za-z0-9\u4e00-\u9fa5]+/g, ' ')
    .trim()
    .split(/\s+/)
    .find(Boolean) || name
  return compact.slice(0, 4)
}

function compactDocTitle(doc) {
  const parts = [
    doc?.shortTitle || doc?.title || 'PDF',
    localized(doc?.lastOpened),
    treeLabel(doc),
    statusLabel(doc?.status, doc),
  ].filter(Boolean)
  return parts.join(' · ')
}

function rootTitle(root) {
  const path = String(root?.path || '')
  const name = localized(root?.name) || path || 'Workspace'
  return path ? `${name} · ${path}` : name
}

function triggerDeleteRoot(root, event = null) {
  if (event) {
    event.preventDefault()
    event.stopPropagation()
  }
  emit('delete-root', root)
}

function triggerDeleteDoc(doc, event = null) {
  if (event) {
    event.preventDefault()
    event.stopPropagation()
  }
  emit('delete-doc', doc)
}

function triggerReindexDoc(doc, event = null) {
  if (event) {
    event.preventDefault()
    event.stopPropagation()
  }
  emit('reindex-doc', doc)
}

// "Add to library" menu: type-agnostic entry (new note / import files / add
// folder) replacing the old PDF-folder-only "+".
const addMenuOpen = ref(false)

function onAddMenuOutsideClick() {
  closeAddMenu()
}

function toggleAddMenu() {
  if (addMenuOpen.value) {
    closeAddMenu()
    return
  }
  addMenuOpen.value = true
  window.addEventListener('click', onAddMenuOutsideClick)
}

function closeAddMenu() {
  addMenuOpen.value = false
  window.removeEventListener('click', onAddMenuOutsideClick)
}

function chooseAdd(kind) {
  closeAddMenu()
  if (kind === 'note') emit('new-note')
  else if (kind === 'files') emit('import-files')
  else emit('add-folder')
}

onBeforeUnmount(() => window.removeEventListener('click', onAddMenuOutsideClick))

</script>

<template>
  <aside
    class="sidebar"
    :class="{ collapsed, 'drag-active': isDropActive }"
    @dragenter="handleDragEnter"
    @dragover="handleDragOver"
    @dragleave="handleDragLeave"
    @drop="handleDrop"
  >
    <div class="sidebar-window-bar" data-tauri-drag-region @mousedown="startWindowDrag">
    </div>

    <template v-if="collapsed">
      <div class="rail-brand" title="Lumenfolio" data-tauri-drag-region @mousedown="startWindowDrag">
        <img :src="lumenfolioLogo" alt="" />
      </div>

      <div class="rail-docs" :aria-label="ui.sources">
        <button
          v-for="doc in visibleRailDocs"
          :key="doc.id"
          type="button"
          class="rail-doc"
          :class="{ active: doc.id === selectedDocId && !trendingActive }"
          :title="compactDocTitle(doc)"
          :aria-label="compactDocTitle(doc)"
          @click="emit('select-doc', doc.id)"
        >
          <span class="rail-doc-icon" aria-hidden="true"></span>
          <span class="rail-doc-name">{{ compactDocLabel(doc) }}</span>
          <span class="rail-doc-status" :title="docStatusTitle(doc)">
            <span class="doc-status-dot" :class="docStatusKind(doc)"></span>
          </span>
          <span v-if="doc.indexStatus === 'indexing'" class="rail-doc-progress" aria-hidden="true">
            <span :style="{ height: `${progressPercent(doc)}%` }"></span>
          </span>
        </button>
        <div v-if="!allDocs.length" class="rail-empty" :title="ui.noSourcesFound">—</div>
      </div>

      <nav class="rail-actions" aria-label="Lumenfolio actions">
        <button
          type="button"
          class="rail-btn"
          :title="ui.addFolder"
          :disabled="isScanning"
          @click="emit('add-folder')"
        >
          +
        </button>
        <button
          type="button"
          class="rail-btn"
          :title="ui.rescanWorkspace"
          :disabled="isScanning || !hasWorkspace"
          @click="emit('rescan')"
        >
          ↻
        </button>
        <button
          type="button"
          class="rail-btn"
          :title="ui.reindexDocument"
          :disabled="isScanning || !selectedDoc || selectedDoc.id === 'empty'"
          @click="emit('reindex-doc')"
        >
          ⟳
        </button>
        <button
          type="button"
          class="rail-btn"
          :title="ui.settings"
          @click="emit('open-settings')"
        >
          ⚙
        </button>
      </nav>
    </template>

    <template v-else>
    <div class="sidebar-expanded">
      <!-- Knowledge-base pivot (P1): Obsidian-style icon rail. Sources (the tree)
           is the default panel; the other icons reuse the existing navigation. -->
      <nav class="sidebar-rail-strip" aria-label="Knowledge base sections">
        <button type="button" class="rail-mode active" :title="ui.sources || 'Sources'" :aria-label="ui.sources || 'Sources'">
          <span aria-hidden="true">📚</span>
        </button>
        <button
          type="button"
          class="rail-mode"
          :title="ui.newNote"
          :aria-label="ui.newNote"
          @click="emit('new-note')"
        ><span aria-hidden="true">📝</span></button>
        <button
          v-if="graphEnabled"
          type="button"
          class="rail-mode"
          :class="{ active: graphActive }"
          :title="ui.knowledgeGraph"
          :aria-label="ui.knowledgeGraph"
          @click="emit('open-graph')"
        ><span aria-hidden="true">🕸</span></button>
        <button
          v-if="trendingEnabled"
          type="button"
          class="rail-mode"
          :class="{ active: trendingActive }"
          :title="ui.trendingPapers"
          :aria-label="ui.trendingPapers"
          @click="emit('open-trending')"
        ><span aria-hidden="true">🔥</span></button>
        <span class="rail-spacer"></span>
        <button type="button" class="rail-mode" :title="ui.settings" :aria-label="ui.settings" @click="emit('open-settings')">
          <span aria-hidden="true">⚙</span>
        </button>
      </nav>

      <div class="sidebar-main">
    <div class="sidebar-header" data-tauri-drag-region @mousedown="startWindowDrag">
      <span class="panel-label">{{ ui.libraryTitle || 'Library' }}</span>
      <div class="panel-actions">
        <div class="add-menu-wrap">
          <button
            type="button"
            class="panel-action-btn"
            :title="ui.addToLibrary || 'Add'"
            :aria-label="ui.addToLibrary || 'Add'"
            :aria-haspopup="true"
            :aria-expanded="addMenuOpen"
            :disabled="isScanning"
            @mousedown.stop
            @click.stop="toggleAddMenu"
          ><span aria-hidden="true">+</span></button>
          <div v-if="addMenuOpen" class="add-menu" @mousedown.stop>
            <button type="button" class="add-menu-item" @click="chooseAdd('note')">
              <span class="add-menu-ic" aria-hidden="true">📝</span>{{ ui.newNote }}
            </button>
            <button type="button" class="add-menu-item" @click="chooseAdd('files')">
              <span class="add-menu-ic" aria-hidden="true">📄</span>{{ ui.importFiles || 'Import files…' }}
            </button>
            <button type="button" class="add-menu-item" @click="chooseAdd('folder')">
              <span class="add-menu-ic" aria-hidden="true">📁</span>{{ ui.addFolder }}
            </button>
          </div>
        </div>
        <button
          type="button"
          class="panel-action-btn"
          :title="ui.rescanWorkspace"
          :aria-label="ui.rescanWorkspace"
          :disabled="isScanning || !hasWorkspace"
          @mousedown.stop
          @click="emit('rescan')"
        ><span aria-hidden="true">↻</span></button>
      </div>
    </div>

    <label class="search-box">
      <span class="search-icon">⌕</span>
      <input
        :value="filter"
        type="text"
        :placeholder="ui.searchPlaceholder"
        @input="emit('update:filter', $event.target.value)"
      />
    </label>

    <div class="tree-area">
      <section
        v-for="workspaceRoot in roots"
        :key="workspaceRoot.id || workspaceRoot.path"
        class="folder-group"
        :class="{ 'drop-target': isDropActive && workspaceRoot.id && workspaceRoot.id === dropTargetRootId }"
        :data-workspace-root-id="workspaceRoot.id"
      >
        <div class="workspace-title-row">
          <button
            type="button"
            class="folder-title"
            :title="rootTitle(workspaceRoot)"
            @click="emit('toggle-root', workspaceRoot.id)"
          >
            <span class="folder-caret">{{ workspaceRoot.collapsed ? '▸' : '▾' }}</span>
            <span class="folder-name">{{ localized(workspaceRoot.name) }}</span>
          </button>
          <button
            type="button"
            class="folder-open-btn"
            :title="ui.addPdfs"
            :aria-label="ui.addPdfs"
            :disabled="isScanning"
            @click="emit('add-pdfs', workspaceRoot.id)"
          >
            +
          </button>
          <button
            type="button"
            class="folder-open-btn"
            :title="ui.openWorkspaceInFileManager"
            :aria-label="ui.openWorkspaceInFileManager"
            @click="emit('open-workspace', workspaceRoot.id)"
          >
            ↗
          </button>
        <button
          type="button"
          class="folder-open-btn folder-delete-btn"
          :title="ui.removeWorkspace"
          :aria-label="ui.removeWorkspace"
          @click="triggerDeleteRoot(workspaceRoot, $event)"
        >
          <span class="folder-action-icon folder-delete-icon" aria-hidden="true">×</span>
        </button>
        </div>
        <div v-if="!workspaceRoot.collapsed" class="workspace-docs">
          <template v-for="folder in workspaceRoot.folders" :key="folder.id">
            <button
              v-for="doc in visibleDocs(folder.docs)"
              :key="doc.id"
              class="doc-row"
              :class="{ active: doc.id === selectedDocId && !trendingActive }"
              :title="compactDocTitle(doc)"
              :draggable="doc.chatReady ? 'true' : 'false'"
              @click="emit('select-doc', doc.id)"
              @dragstart="handleDocDragStart($event, doc)"
            >
              <div class="doc-main">
                <span class="doc-name-wrap">
                  <span
                    class="doc-status-dot"
                    :class="docStatusKind(doc)"
                    :title="docStatusTitle(doc)"
                    :aria-label="docStatusTitle(doc)"
                  ></span>
                  <span class="doc-name">{{ doc.shortTitle }}</span>
                </span>
                <span class="doc-time">{{ localized(doc.lastOpened) }}</span>
              </div>
              <div v-if="doc.indexStatus === 'indexing'" class="doc-progress" aria-hidden="true">
                <span :style="{ width: `${progressPercent(doc)}%` }"></span>
              </div>
              <span class="doc-row-actions">
                <span
                  class="doc-action-btn"
                  role="button"
                  tabindex="0"
                  :title="ui.reindexDocument"
                  :aria-label="ui.reindexDocument"
                  @click.stop="triggerReindexDoc(doc, $event)"
                  @keydown.enter.stop.prevent="triggerReindexDoc(doc, $event)"
                >⟳</span>
                <span
                  class="doc-action-btn doc-delete-btn"
                  role="button"
                  tabindex="0"
                  :title="ui.deleteDocument"
                  :aria-label="ui.deleteDocument"
                  @click.stop="triggerDeleteDoc(doc, $event)"
                  @keydown.enter.stop.prevent="triggerDeleteDoc(doc, $event)"
                >×</span>
              </span>
            </button>
          </template>
        </div>
      </section>

      <div v-if="!allDocs.length" class="empty-tree">
        {{ ui.noSourcesFound }}
      </div>
    </div>

    <div v-if="scanError" class="scan-error">{{ scanError }}</div>

    <div class="sidebar-status">
      <span v-if="scanStatus === 'scanning'">{{ ui.scanningWorkspace }}</span>
      <span v-else-if="allDocs.length">{{ allDocs.length }} {{ ui.sourcesCountLabel || 'sources' }}</span>
    </div>
      </div>
    </div>
    </template>
  </aside>
</template>

<style scoped>
.sidebar {
  position: relative;
  z-index: 6;
  width: 296px;
  min-width: 296px;
  background: var(--bg-sidebar);
  border-right: 1px solid var(--line-soft);
  display: flex;
  flex-direction: column;
  padding: 0 14px 16px;
  gap: 14px;
  transition: width 180ms ease, min-width 180ms ease, padding 180ms ease;
}

/* Knowledge-base pivot (P1): icon rail + panel inside the expanded sidebar. */
.sidebar-expanded {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 10px;
}

.sidebar-rail-strip {
  flex: 0 0 auto;
  width: 34px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding-top: 6px;
}

.rail-mode {
  width: 32px;
  height: 32px;
  border-radius: 8px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 15px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.rail-mode:hover {
  background: rgba(255, 255, 255, 0.05);
  color: var(--text-primary);
}

.rail-mode.active {
  background: var(--accent-soft, rgba(106, 169, 255, 0.14));
  border-color: rgba(106, 169, 255, 0.3);
  color: var(--accent, #6aa9ff);
}

.rail-spacer {
  flex: 1;
}

.sidebar-main {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.sidebar.drag-active {
  background: linear-gradient(145deg, rgba(106, 169, 255, 0.12), transparent 70%);
  border-right-color: rgba(106, 169, 255, 0.26);
}

.sidebar.drag-active::after {
  content: '';
  position: absolute;
  inset: 6px;
  border: 1px dashed rgba(132, 183, 255, 0.75);
  border-radius: 12px;
  pointer-events: none;
}

.sidebar.collapsed {
  width: 104px;
  min-width: 104px;
  padding: 0 12px 12px;
  align-items: center;
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.028), transparent 150px),
    var(--bg-sidebar);
}

.sidebar-window-bar {
  position: relative;
  /* Just enough to clear the macOS traffic lights (≈y20–34); was 56+14≈70px,
     which left a large empty gap above the brand. */
  min-height: 40px;
  width: 100%;
  flex-shrink: 0;
  display: flex;
  align-items: flex-start;
  justify-content: flex-end;
  padding-top: 0;
}

.sidebar.collapsed .sidebar-window-bar {
  min-height: 48px;
  padding-top: 0;
}

.rail-brand {
  width: 58px;
  height: 58px;
  display: grid;
  place-items: center;
  flex: 0 0 auto;
  margin-top: -2px;
  border-radius: 16px;
  background: rgba(255, 255, 255, 0.025);
}

.rail-brand img {
  width: 48px;
  height: 48px;
  object-fit: contain;
  filter: drop-shadow(0 2px 4px rgba(0, 0, 0, 0.24));
}

.rail-docs {
  width: 100%;
  min-height: 0;
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  overflow: auto;
  padding: 6px 0 10px;
}

.rail-docs::-webkit-scrollbar {
  width: 0;
  height: 0;
}

.rail-doc {
  position: relative;
  width: 70px;
  min-height: 74px;
  flex: 0 0 auto;
  display: grid;
  grid-template-rows: 28px auto 8px;
  justify-items: center;
  align-content: center;
  gap: 4px;
  border: 1px solid transparent;
  border-radius: 14px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  padding: 8px 7px 7px;
  text-align: center;
  transition: border-color 140ms ease, background 140ms ease, color 140ms ease;
}

.rail-doc:hover {
  background: rgba(255, 255, 255, 0.045);
  color: var(--text-primary);
}

.rail-doc.active {
  color: var(--text-primary);
  border-color: rgba(106, 169, 255, 0.44);
  background: rgba(106, 169, 255, 0.12);
}

.rail-doc-icon {
  position: relative;
  width: 25px;
  height: 30px;
  border-radius: 8px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  background:
    linear-gradient(135deg, transparent 0 8px, rgba(255, 255, 255, 0.12) 8px 9px, transparent 9px) top right / 12px 12px no-repeat,
    linear-gradient(180deg, rgba(255, 255, 255, 0.11), rgba(255, 255, 255, 0.045));
}

.rail-doc-icon::before {
  content: "";
  position: absolute;
  right: -1px;
  top: -1px;
  width: 10px;
  height: 10px;
  border-left: 1px solid rgba(255, 255, 255, 0.14);
  border-bottom: 1px solid rgba(255, 255, 255, 0.14);
  border-radius: 0 7px 0 4px;
  background: rgba(255, 255, 255, 0.08);
}

.rail-doc-icon::after {
  content: "";
  position: absolute;
  left: 6px;
  right: 6px;
  top: 13px;
  height: 8px;
  border-top: 2px solid rgba(235, 241, 248, 0.58);
  border-bottom: 2px solid rgba(235, 241, 248, 0.42);
}

.rail-doc-name {
  width: 100%;
  min-width: 0;
  color: inherit;
  font-size: 11px;
  line-height: 1.15;
  font-weight: 700;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.rail-doc-status {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 8px;
}

.rail-doc-progress {
  position: absolute;
  right: 5px;
  top: 8px;
  bottom: 8px;
  width: 3px;
  overflow: hidden;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.08);
}

.rail-doc-progress span {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  border-radius: inherit;
  background: linear-gradient(180deg, #8ae8ff, #ffd089);
}

.rail-empty {
  width: 70px;
  min-height: 56px;
  display: grid;
  place-items: center;
  border: 1px dashed rgba(255, 255, 255, 0.14);
  border-radius: 14px;
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 800;
}

.rail-actions {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  width: 100%;
  flex: 0 0 auto;
  padding-top: 10px;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
}

.rail-btn {
  width: 38px;
  height: 38px;
  border-radius: 12px;
  border: 1px solid rgba(255, 255, 255, 0.13);
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.085), rgba(255, 255, 255, 0.035)),
    rgba(255, 255, 255, 0.028);
  color: rgba(235, 241, 248, 0.84);
  cursor: pointer;
  display: grid;
  place-items: center;
  font-size: 18px;
  font-weight: 750;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.08),
    0 8px 24px rgba(0, 0, 0, 0.14);
  transition: border-color 140ms ease, background 140ms ease, color 140ms ease, transform 140ms ease;
}

.rail-btn:hover:not(:disabled) {
  color: var(--text-primary);
  border-color: rgba(122, 162, 255, 0.58);
  background:
    linear-gradient(180deg, rgba(122, 162, 255, 0.18), rgba(122, 162, 255, 0.065)),
    rgba(255, 255, 255, 0.045);
  transform: translateY(-1px);
}

.rail-btn:disabled {
  opacity: 0.42;
  cursor: not-allowed;
}

.sidebar-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

.panel-label {
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--text-muted, var(--text-secondary));
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.panel-actions {
  display: flex;
  gap: 2px;
  flex: 0 0 auto;
}

.panel-action-btn {
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--text-secondary);
  font-size: 15px;
  line-height: 1;
  cursor: pointer;
  transition: background 0.12s ease, color 0.12s ease;
}

.panel-action-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-primary);
}

.panel-action-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.add-menu-wrap {
  position: relative;
}

.add-menu {
  position: absolute;
  top: calc(100% + 4px);
  right: 0;
  z-index: 20;
  min-width: 168px;
  padding: 4px;
  border: 1px solid var(--line-soft);
  border-radius: 10px;
  background: var(--bg-panel, #1f1f24);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.32);
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.add-menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 7px 9px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--text-primary);
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}

.add-menu-item:hover {
  background: rgba(255, 255, 255, 0.06);
}

.add-menu-ic {
  font-size: 14px;
  line-height: 1;
}

.path-reveal-btn {
  width: 22px;
  height: 22px;
  flex: 0 0 22px;
  border-radius: 7px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  display: grid;
  place-items: center;
  padding: 0;
  font-size: 12px;
  line-height: 1;
  opacity: 0;
  transition: opacity 140ms ease, background 140ms ease, color 140ms ease, border-color 140ms ease;
}

.sidebar-header:hover .path-reveal-btn,
.path-reveal-btn:focus-visible {
  opacity: 1;
}

.path-reveal-btn:hover:not(:disabled) {
  color: var(--text-primary);
  border-color: rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.055);
}

.path-reveal-btn:disabled {
  cursor: not-allowed;
  opacity: 0;
}


.search-box {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid var(--line-soft);
  background: rgba(255, 255, 255, 0.03);
  border-radius: 12px;
  padding: 0 12px;
  min-height: 40px;
}

.search-box input {
  width: 100%;
  background: transparent;
  border: none;
  color: var(--text-primary);
  outline: none;
  font-size: 13px;
}

.search-icon,
.tree-area {
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  padding-right: 4px;
}

/* "Trending Papers" discovery entry, pinned above the local folders. */
/* Scrollbar look comes from the global style in styles/main.css. */

.folder-group + .folder-group {
  margin-top: 0px;
}

.folder-group {
  position: relative;
  border-radius: 14px;
  padding: 0px 4px 0px;
  margin-top: -6px;
  transition: background 140ms ease, box-shadow 140ms ease;
}

.folder-group.drop-target {
  background: rgba(106, 169, 255, 0.14);
  box-shadow:
    inset 0 0 0 1px rgba(132, 183, 255, 0.55),
    0 0 0 1px rgba(132, 183, 255, 0.2);
}

.folder-group.drop-target .folder-title {
  color: var(--text-primary);
}

.workspace-title-row {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 8px;
}

.folder-title {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  padding: 4px 2px;
  text-align: left;
  font-size: 12px;
  font-weight: 760;
}

.folder-title:hover {
  color: var(--text-primary);
}

.folder-caret {
  width: 12px;
  flex: 0 0 12px;
  color: var(--text-muted);
}

.folder-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.folder-open-btn {
  width: 24px;
  height: 24px;
  flex: 0 0 24px;
  display: grid;
  place-items: center;
  border: 1px solid transparent;
  border-radius: 7px;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  opacity: 0;
  transition: opacity 140ms ease, background 140ms ease, color 140ms ease, border-color 140ms ease;
  padding: 0;
  line-height: 1;
  font-size: 14px;
}

.folder-action-icon {
  display: block;
  line-height: 1;
  pointer-events: none;
}

.workspace-title-row:hover .folder-open-btn,
.folder-open-btn:focus-visible {
  opacity: 1;
}

.folder-open-btn:hover {
  color: var(--text-primary);
  border-color: rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.055);
}

.folder-delete-btn {
  color: rgba(255, 102, 102, 0.9);
}

.folder-delete-icon {
  position: relative;
  left: 0;
}

.folder-delete-btn:hover {
  color: #ff6b6b;
  border-color: rgba(255, 107, 107, 0.4);
  background: rgba(255, 107, 107, 0.12);
}

.doc-row {
  width: 100%;
  background: transparent;
  border: none;
  color: inherit;
  text-align: left;
  cursor: pointer;
}

.doc-row {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  border-radius: 12px;
  margin-bottom: 6px;
  border: 1px solid transparent;
  /* WKWebView (Tauri/Safari) sizes <button> to its min-content width and won't
     honor width:100% shrink, breaking the ellipsis chain. Force shrink + clip. */
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
}

/* Per-document actions (reindex + delete), revealed on row hover. */
.doc-row-actions {
  position: absolute;
  top: 7px;
  right: 8px;
  display: flex;
  gap: 2px;
  opacity: 0;
  transition: opacity 0.12s ease;
}

.doc-row:hover .doc-row-actions,
.doc-row-actions:focus-within {
  opacity: 1;
}

.doc-action-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 6px;
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 1;
  cursor: pointer;
  transition: background 0.12s ease, color 0.12s ease;
}

.doc-action-btn:hover {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-primary);
}

.doc-delete-btn {
  color: rgba(255, 102, 102, 0.9);
}

.doc-delete-btn:hover {
  background: rgba(255, 102, 102, 0.14);
  color: rgba(255, 102, 102, 1);
}

.doc-row:hover {
  background: rgba(255, 255, 255, 0.04);
}

.doc-row.active {
  background: rgba(255, 255, 255, 0.075);
  border-color: rgba(106, 169, 255, 0.25);
}

.doc-main {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
}

.doc-name-wrap {
  display: flex;
  align-items: baseline;
  gap: 8px;
  flex: 1;
  min-width: 0;
}

.doc-status-dot {
  flex-shrink: 0;
  align-self: center;
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: var(--text-muted);
}

.doc-status-dot.ready {
  background: #3db570;
  box-shadow: 0 0 0 3px rgba(61, 181, 112, 0.16);
}

.doc-status-dot.failed {
  background: #e06464;
  box-shadow: 0 0 0 3px rgba(198, 73, 73, 0.16);
}

.doc-status-dot.processing {
  background: #f0b54a;
  animation: doc-status-pulse 1.4s ease-in-out infinite;
}

@keyframes doc-status-pulse {
  0%,
  100% {
    box-shadow: 0 0 0 0 rgba(240, 181, 74, 0.45);
    opacity: 1;
  }
  50% {
    box-shadow: 0 0 0 4px rgba(240, 181, 74, 0);
    opacity: 0.55;
  }
}

.doc-name {
  flex: 1;
  min-width: 0;
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.35;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.doc-time {
  flex-shrink: 0;
  color: var(--text-muted);
  font-size: 12px;
  white-space: nowrap;
}

.doc-progress {
  height: 3px;
  overflow: hidden;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.08);
}

.doc-progress span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: linear-gradient(90deg, #ffd089, #8ae8ff);
  transition: width 180ms ease;
}

.empty-tree,
.scan-error {
  border: 1px solid var(--line-soft);
  border-radius: 12px;
  padding: 12px;
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 1.5;
}

.scan-error {
  color: #ffb3b3;
  border-color: rgba(198, 73, 73, 0.28);
  background: rgba(198, 73, 73, 0.1);
}

.sidebar-status {
  flex: 0 0 auto;
  padding: 6px 10px 2px;
  font-size: 11px;
  color: var(--text-muted, var(--text-secondary));
  min-height: 16px;
}
</style>
