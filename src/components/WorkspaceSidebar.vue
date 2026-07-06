<script setup>
import { computed, reactive, ref, watch, onBeforeUnmount } from 'vue'
import lumenfolioLogo from '../assets/lumenfolio-logo-transparent.png'
import { startWindowDrag } from '../windowDrag'

const props = defineProps({
  roots: {
    type: Array,
    default: () => [],
  },
  // Knowledge-base pivot: logical collections (flat list of
  // { id, parentId, name, position }) + the full document pool. When present,
  // the expanded tree groups documents by collection instead of by disk root.
  collections: {
    type: Array,
    default: () => [],
  },
  documents: {
    type: Array,
    default: () => [],
  },
  selectedCollectionId: {
    type: [String, null],
    default: null,
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
  // Collection id (or '__unfiled__') the OS-file drag is currently hovering.
  dropTargetCollectionId: {
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
  'new-collection',
  'rename-collection',
  'delete-collection',
  'select-collection',
  'move-doc-to-collection',
  'move-collection',
])

// C-d: which collection row an internal (doc/collection) drag is hovering.
const dragOverCollectionId = ref(null)

function handleCollectionDragStart(event, collection) {
  if (!event?.dataTransfer) return
  event.dataTransfer.setData('application/x-lumenfolio-collection-id', collection.id)
  event.dataTransfer.effectAllowed = 'move'
}

function handleCollectionDragOver(event, collectionId) {
  const dt = event.dataTransfer
  if (!dt) return
  const draggingDoc = dt.types.includes('application/x-lumenfolio-doc-id')
  const draggingCollection = dt.types.includes('application/x-lumenfolio-collection-id')
  if (!draggingDoc && !draggingCollection) return
  event.preventDefault()
  dt.dropEffect = 'move'
  dragOverCollectionId.value = collectionId
}

function handleCollectionDragLeave(collectionId) {
  if (dragOverCollectionId.value === collectionId) {
    dragOverCollectionId.value = null
  }
}

// Whether `candidateId` is `ancestorId` itself or inside its subtree — used to
// reject dropping a collection into its own descendant before hitting the
// backend (which would reject the cycle with a raw error).
function isCollectionInSubtree(candidateId, ancestorId) {
  if (candidateId === ancestorId) return true
  const childrenOf = collectionChildren.value
  const seen = new Set()
  const stack = [...(childrenOf.get(ancestorId) || [])]
  while (stack.length) {
    const node = stack.pop()
    if (!node || seen.has(node.id)) continue
    seen.add(node.id)
    if (node.id === candidateId) return true
    stack.push(...(childrenOf.get(node.id) || []))
  }
  return false
}

function handleCollectionDrop(event, collectionId) {
  const dt = event.dataTransfer
  dragOverCollectionId.value = null
  if (!dt) return
  const docId = dt.getData('application/x-lumenfolio-doc-id')
  const movedCollectionId = dt.getData('application/x-lumenfolio-collection-id')
  const target = collectionId === '__unfiled__' ? null : collectionId
  if (docId) {
    event.preventDefault()
    emit('move-doc-to-collection', { docId, collectionId: target })
  } else if (movedCollectionId && movedCollectionId !== collectionId) {
    // Reject self / descendant targets here so the backend never has to.
    if (target !== null && isCollectionInSubtree(collectionId, movedCollectionId)) return
    event.preventDefault()
    emit('move-collection', { id: movedCollectionId, parentId: target })
  }
}

const normalizedFilter = computed(() => String(props.filter || '').trim().toLowerCase())
const hasWorkspace = computed(() => props.roots.some((root) => Boolean(String(root.path || '').trim())))
const isScanning = computed(() => props.scanStatus === 'choosing' || props.scanStatus === 'scanning')
// Knowledge-base pivot: prefer the flat document pool (props.documents). Fall
// back to the legacy roots→folders→docs derivation so the rail keeps working if
// documents haven't been wired up.
const rootDocs = computed(() => props.roots.flatMap((root) => (
  (root.folders || []).flatMap((folder) => folder.docs || [])
)))
const allDocs = computed(() => (
  props.documents.length ? props.documents : rootDocs.value
))
const visibleRailDocs = computed(() => (
  props.documents.length
    ? props.documents
    : props.roots.flatMap((root) => (
      root.collapsed ? [] : (root.folders || []).flatMap((folder) => folder.docs || [])
    ))
))
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
  // Any source can be dragged to refile it — even one still indexing (readiness
  // gates asking, not organizing).
  if (!doc?.id || !event?.dataTransfer) return
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

// ── Collection tree (knowledge-base pivot) ──────────────────────────────────
// Documents are grouped by collectionId; collections nest by parentId. The tree
// is rendered as a flattened, depth-indented list of rows honoring a local
// expand/collapse Set (top-level collections default to expanded).

const UNFILED_ID = '__unfiled__'
const expandedCollections = reactive(new Set())
// Track which collection ids we've already seeded so newly-created top-level
// collections default to expanded without re-expanding ones the user collapsed.
const seededCollectionIds = new Set()

watch(
  () => props.collections,
  (list) => {
    for (const collection of list || []) {
      if (seededCollectionIds.has(collection.id)) continue
      seededCollectionIds.add(collection.id)
      if (collection.parentId == null) expandedCollections.add(collection.id)
    }
  },
  { immediate: true, deep: true },
)

// Map parentId → children (in position order), for building the flattened tree.
const collectionChildren = computed(() => {
  const byParent = new Map()
  for (const collection of props.collections || []) {
    const key = collection.parentId ?? null
    if (!byParent.has(key)) byParent.set(key, [])
    byParent.get(key).push(collection)
  }
  for (const children of byParent.values()) {
    children.sort((a, b) => {
      const posA = Number(a.position ?? 0)
      const posB = Number(b.position ?? 0)
      if (posA !== posB) return posA - posB
      return String(a.name || '').localeCompare(String(b.name || ''))
    })
  }
  return byParent
})

// Documents keyed by collection id. Docs whose collectionId is null/undefined or
// points at a non-existent collection fall into the Unfiled bucket.
const docsByCollection = computed(() => {
  const known = new Set((props.collections || []).map((c) => c.id))
  const map = new Map()
  for (const doc of allDocs.value) {
    const raw = doc?.collectionId
    const key = raw != null && known.has(raw) ? raw : UNFILED_ID
    if (!map.has(key)) map.set(key, [])
    map.get(key).push(doc)
  }
  return map
})

function collectionDocCount(id) {
  return (docsByCollection.value.get(id) || []).length
}

function isCollectionExpanded(id) {
  return expandedCollections.has(id)
}

function toggleCollectionExpanded(id) {
  if (expandedCollections.has(id)) expandedCollections.delete(id)
  else expandedCollections.add(id)
}

// Flatten the collection forest into an ordered array of rows. Each collection
// row is followed (when expanded) by its child collections then its documents,
// all depth-indented. The Unfiled bucket is always appended at the bottom.
const treeRows = computed(() => {
  const rows = []
  const childrenOf = collectionChildren.value
  // Guards against a hypothetical parent-id cycle and lets us surface orphans.
  const seen = new Set()

  const pushCollection = (collection, depth) => {
    seen.add(collection.id)
    rows.push({ type: 'collection', depth, collection })
    if (isCollectionExpanded(collection.id)) {
      walk(collection.id, depth + 1)
      for (const doc of visibleDocs(docsByCollection.value.get(collection.id) || [])) {
        rows.push({ type: 'doc', depth: depth + 1, doc })
      }
    }
  }

  const walk = (parentId, depth) => {
    const children = childrenOf.get(parentId ?? null) || []
    for (const collection of children) {
      if (seen.has(collection.id)) continue
      pushCollection(collection, depth)
    }
  }
  walk(null, 0)

  // Defensive: a collection unreachable from the root (dangling parent id, or a
  // cycle) would otherwise never render — and its documents, bucketed under it
  // by docsByCollection, would vanish. Surface any such orphan at the top level.
  for (const collection of props.collections || []) {
    if (!seen.has(collection.id)) {
      pushCollection(collection, 0)
    }
  }

  // Unfiled node — only shown when it actually holds un-filed sources, so an
  // empty inbox doesn't read as mystery clutter. It is not a user collection and
  // has no delete/rename; sources leave it by being filed into a collection or
  // deleted.
  if ((docsByCollection.value.get(UNFILED_ID) || []).length > 0) {
    rows.push({ type: 'collection', depth: 0, unfiled: true })
    if (isCollectionExpanded(UNFILED_ID)) {
      for (const doc of visibleDocs(docsByCollection.value.get(UNFILED_ID) || [])) {
        rows.push({ type: 'doc', depth: 1, doc })
      }
    }
  }
  return rows
})

// Unfiled starts expanded.
expandedCollections.add(UNFILED_ID)

function rowIndentStyle(depth) {
  return { paddingLeft: `${depth * 14}px` }
}

// Inline rename: which collection row is being edited + its draft text.
const renamingCollectionId = ref('')
const renameDraft = ref('')

function startRenameCollection(collection, event = null) {
  if (event) {
    event.preventDefault()
    event.stopPropagation()
  }
  renamingCollectionId.value = collection.id
  renameDraft.value = collection.name || ''
}

function commitRenameCollection() {
  const id = renamingCollectionId.value
  const name = String(renameDraft.value || '').trim()
  renamingCollectionId.value = ''
  if (id && name) emit('rename-collection', { id, name })
}

function cancelRenameCollection() {
  renamingCollectionId.value = ''
  renameDraft.value = ''
}

// Autofocus + select the inline rename input when it mounts.
const vFocus = {
  mounted(el) {
    el.focus()
    el.select?.()
  },
}

function triggerNewSubcollection(collection, event = null) {
  if (event) {
    event.preventDefault()
    event.stopPropagation()
  }
  // Ensure the parent is expanded so the new child is visible.
  expandedCollections.add(collection.id)
  emit('new-collection', collection.id)
}

// Inline two-step delete confirm. `window.confirm` is unreliable in the Tauri
// webview (returns false on WKWebView), so the row swaps its actions for a
// ✓/✗ pair instead of popping a native dialog. Delete is non-destructive:
// sources drop to Unfiled, files on disk are untouched.
const confirmDeleteId = ref(null)

function requestDeleteCollection(collection, event = null) {
  if (event) {
    event.preventDefault()
    event.stopPropagation()
  }
  confirmDeleteId.value = collection.id
}

function confirmDeleteCollection(collection, event = null) {
  if (event) {
    event.preventDefault()
    event.stopPropagation()
  }
  confirmDeleteId.value = null
  emit('delete-collection', collection.id)
}

function cancelDeleteCollection(event = null) {
  if (event) {
    event.preventDefault()
    event.stopPropagation()
  }
  confirmDeleteId.value = null
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

      <!-- Knowledge-base pivot: the collapsed rail mirrors the expanded section
           ribbon (one mental model). Sources expands the sidebar; the rest reuse
           the existing navigation. -->
      <nav class="sidebar-rail-strip collapsed-strip" aria-label="Knowledge base sections">
        <button
          type="button"
          class="rail-mode"
          :class="{ active: !trendingActive && !graphActive }"
          :title="ui.sources || 'Sources'"
          :aria-label="ui.sources || 'Sources'"
          @click="emit('toggle-collapse')"
        ><span aria-hidden="true">📚</span></button>
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
      </nav>

      <div class="rail-divider"></div>

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
      </div>

      <nav class="rail-actions" aria-label="Lumenfolio actions">
        <div class="add-menu-wrap">
          <button
            type="button"
            class="rail-btn"
            :title="ui.addToLibrary || 'Add'"
            :aria-label="ui.addToLibrary || 'Add'"
            :aria-haspopup="true"
            :aria-expanded="addMenuOpen"
            :disabled="isScanning"
            @mousedown.stop
            @click.stop="toggleAddMenu"
          >
            +
          </button>
          <div v-if="addMenuOpen" class="add-menu collapsed-add-menu" @mousedown.stop>
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
          class="rail-btn"
          :title="ui.settings"
          :aria-label="ui.settings"
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
          :title="ui.newCollection || 'New collection'"
          :aria-label="ui.newCollection || 'New collection'"
          @mousedown.stop
          @click="emit('new-collection', null)"
        ><span aria-hidden="true">🗂</span></button>
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
      <!-- Knowledge-base pivot: collection tree. Flattened, depth-indented rows
           mixing collection folders and their documents; an always-present
           "Unfiled" node at the bottom holds documents with no collection. -->
      <div class="collection-tree">
        <template v-for="row in treeRows" :key="row.type === 'doc' ? `doc-${row.doc.id}` : (row.unfiled ? 'col-unfiled' : `col-${row.collection.id}`)">
          <!-- Unfiled node -->
          <div
            v-if="row.type === 'collection' && row.unfiled"
            class="collection-row unfiled-row"
            :class="{ 'drag-over': dragOverCollectionId === '__unfiled__' || dropTargetCollectionId === '__unfiled__' }"
            :style="rowIndentStyle(row.depth)"
            data-collection-id="__unfiled__"
            @dragover="handleCollectionDragOver($event, '__unfiled__')"
            @dragleave="handleCollectionDragLeave('__unfiled__')"
            @drop="handleCollectionDrop($event, '__unfiled__')"
          >
            <button
              type="button"
              class="collection-caret"
              :aria-label="ui.unfiled || 'Unfiled'"
              @click="toggleCollectionExpanded('__unfiled__')"
            >{{ isCollectionExpanded('__unfiled__') ? '▾' : '▸' }}</button>
            <span class="collection-name">{{ ui.unfiled || 'Unfiled' }}</span>
            <span class="collection-count">{{ collectionDocCount('__unfiled__') }}</span>
          </div>

          <!-- Collection node -->
          <div
            v-else-if="row.type === 'collection'"
            class="collection-row"
            :class="{
              active: row.collection.id === selectedCollectionId,
              'drag-over': dragOverCollectionId === row.collection.id || dropTargetCollectionId === row.collection.id,
            }"
            :style="rowIndentStyle(row.depth)"
            :data-collection-id="row.collection.id"
            :draggable="renamingCollectionId === row.collection.id ? 'false' : 'true'"
            @dragstart="handleCollectionDragStart($event, row.collection)"
            @dragover="handleCollectionDragOver($event, row.collection.id)"
            @dragleave="handleCollectionDragLeave(row.collection.id)"
            @drop="handleCollectionDrop($event, row.collection.id)"
          >
            <button
              type="button"
              class="collection-caret"
              :aria-label="isCollectionExpanded(row.collection.id) ? ui.collapse : ui.expand"
              @click.stop="toggleCollectionExpanded(row.collection.id)"
            >{{ isCollectionExpanded(row.collection.id) ? '▾' : '▸' }}</button>
            <template v-if="renamingCollectionId === row.collection.id">
              <input
                v-model="renameDraft"
                class="collection-rename-input"
                type="text"
                @click.stop
                @keydown.enter.prevent="commitRenameCollection"
                @keydown.esc.prevent="cancelRenameCollection"
                @blur="commitRenameCollection"
                v-focus
              />
            </template>
            <template v-else>
              <button
                type="button"
                class="collection-name-btn"
                :title="row.collection.name"
                @click="emit('select-collection', row.collection.id)"
              >
                <span class="collection-name">{{ row.collection.name }}</span>
              </button>
              <span class="collection-count">{{ collectionDocCount(row.collection.id) }}</span>
              <span v-if="confirmDeleteId === row.collection.id" class="collection-actions collection-confirm">
                <button
                  type="button"
                  class="collection-action-btn collection-confirm-yes"
                  :title="ui.deleteCollectionConfirm || 'Delete? Sources move to Unfiled.'"
                  :aria-label="ui.deleteCollection || 'Delete'"
                  @click.stop="confirmDeleteCollection(row.collection, $event)"
                >✓</button>
                <button
                  type="button"
                  class="collection-action-btn"
                  :title="ui.cancel || 'Cancel'"
                  :aria-label="ui.cancel || 'Cancel'"
                  @click.stop="cancelDeleteCollection($event)"
                >✗</button>
              </span>
              <span v-else class="collection-actions">
                <button
                  type="button"
                  class="collection-action-btn"
                  :title="ui.newSubcollection || 'New sub-collection'"
                  :aria-label="ui.newSubcollection || 'New sub-collection'"
                  @click.stop="triggerNewSubcollection(row.collection, $event)"
                >+</button>
                <button
                  type="button"
                  class="collection-action-btn"
                  :title="ui.renameCollection || 'Rename'"
                  :aria-label="ui.renameCollection || 'Rename'"
                  @click.stop="startRenameCollection(row.collection, $event)"
                >✎</button>
                <button
                  type="button"
                  class="collection-action-btn collection-delete-btn"
                  :title="ui.deleteCollection || 'Delete'"
                  :aria-label="ui.deleteCollection || 'Delete'"
                  @click.stop="requestDeleteCollection(row.collection, $event)"
                >×</button>
              </span>
            </template>
          </div>

          <!-- Document row (indented under its collection) -->
          <button
            v-else
            class="doc-row"
            :class="{ active: row.doc.id === selectedDocId && !trendingActive }"
            :style="rowIndentStyle(row.depth)"
            :title="compactDocTitle(row.doc)"
            draggable="true"
            @click="emit('select-doc', row.doc.id)"
            @dragstart="handleDocDragStart($event, row.doc)"
          >
            <div class="doc-main">
              <span class="doc-name-wrap">
                <span
                  class="doc-status-dot"
                  :class="docStatusKind(row.doc)"
                  :title="docStatusTitle(row.doc)"
                  :aria-label="docStatusTitle(row.doc)"
                ></span>
                <span class="doc-name">{{ row.doc.shortTitle }}</span>
              </span>
              <span class="doc-time">{{ localized(row.doc.lastOpened) }}</span>
            </div>
            <div v-if="row.doc.indexStatus === 'indexing'" class="doc-progress" aria-hidden="true">
              <span :style="{ width: `${progressPercent(row.doc)}%` }"></span>
            </div>
            <span class="doc-row-actions">
              <span
                class="doc-action-btn"
                role="button"
                tabindex="0"
                :title="ui.reindexDocument"
                :aria-label="ui.reindexDocument"
                @click.stop="triggerReindexDoc(row.doc, $event)"
                @keydown.enter.stop.prevent="triggerReindexDoc(row.doc, $event)"
              >⟳</span>
              <span
                class="doc-action-btn doc-delete-btn"
                role="button"
                tabindex="0"
                :title="ui.deleteDocument"
                :aria-label="ui.deleteDocument"
                @click.stop="triggerDeleteDoc(row.doc, $event)"
                @keydown.enter.stop.prevent="triggerDeleteDoc(row.doc, $event)"
              >×</span>
            </span>
          </button>
        </template>
      </div>

      <div v-if="!allDocs.length && !collections.length" class="empty-tree">
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

/* Collapsed rail: the section ribbon fills the width and its icons read a bit
   larger than in the thin expanded strip. */
.collapsed-strip {
  width: 100%;
  gap: 6px;
  padding-top: 4px;
}

.collapsed-strip .rail-mode {
  width: 40px;
  height: 40px;
  font-size: 20px;
  border-radius: 11px;
}

.rail-divider {
  flex: 0 0 auto;
  width: 26px;
  height: 1px;
  background: rgba(255, 255, 255, 0.08);
  margin: 8px 0 2px;
}

/* Add menu pops to the right of the narrow rail button (not below it). */
.collapsed-add-menu {
  top: auto;
  bottom: 0;
  right: auto;
  left: calc(100% + 6px);
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

/* Knowledge-base pivot: collection tree rows. */
.collection-tree {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.collection-row {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
  border-radius: 8px;
  padding: 4px 6px 4px 2px;
  border: 1px solid transparent;
  color: var(--text-secondary);
}

.collection-row:hover {
  background: rgba(255, 255, 255, 0.04);
}

.collection-row.active {
  background: rgba(106, 169, 255, 0.12);
  border-color: rgba(106, 169, 255, 0.25);
  color: var(--text-primary);
}

/* C-d: highlight a collection as a drop target (doc move / nest / OS file). */
.collection-row.drag-over {
  background: rgba(106, 169, 255, 0.2);
  border-color: rgba(106, 169, 255, 0.55);
  color: var(--text-primary);
}

.collection-caret {
  width: 14px;
  flex: 0 0 14px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  padding: 0;
  font-size: 11px;
  line-height: 1;
}

.collection-name-btn {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  border: none;
  background: transparent;
  color: inherit;
  cursor: pointer;
  padding: 2px 0;
  text-align: left;
}

.collection-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  font-weight: 700;
}

.unfiled-row .collection-name {
  flex: 1;
  color: var(--text-muted);
}

.collection-count {
  flex: 0 0 auto;
  color: var(--text-muted);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}

.collection-actions {
  display: flex;
  gap: 2px;
  flex: 0 0 auto;
  opacity: 0;
  transition: opacity 0.12s ease;
}

.collection-row:hover .collection-actions,
.collection-actions:focus-within,
.collection-actions.collection-confirm {
  opacity: 1;
}

/* Inline delete confirm: keep the ✓/✗ visible regardless of hover. */
.collection-confirm-yes {
  color: var(--danger, #e06a6a);
}

.collection-action-btn {
  width: 18px;
  height: 18px;
  display: grid;
  place-items: center;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1;
  cursor: pointer;
  transition: background 0.12s ease, color 0.12s ease;
}

.collection-action-btn:hover {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-primary);
}

.collection-delete-btn {
  color: rgba(255, 102, 102, 0.9);
}

.collection-delete-btn:hover {
  background: rgba(255, 102, 102, 0.14);
  color: rgba(255, 102, 102, 1);
}

.collection-rename-input {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--line-soft);
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-primary);
  font-size: 12px;
  font-weight: 700;
  padding: 2px 6px;
  outline: none;
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
