<script setup>
import { computed, nextTick, onActivated, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import * as pdfjsLib from 'pdfjs-dist/legacy/build/pdf.mjs'
import pdfWorkerUrl from 'pdfjs-dist/legacy/build/pdf.worker.mjs?url'
import {
  EventBus,
  PDFLinkService,
  PDFViewer,
} from 'pdfjs-dist/legacy/web/pdf_viewer.mjs'
import 'pdfjs-dist/legacy/web/pdf_viewer.css'

pdfjsLib.GlobalWorkerOptions.workerSrc = pdfWorkerUrl

const MIN_SCALE = 0.7
const MAX_SCALE = 2.4
const DEFAULT_SCALE = 1.18
const { AnnotationEditorParamsType, AnnotationEditorType } = pdfjsLib
const SELECT_TOOL = 'select'
// Keep PDF.js in an editing mode while the logical Select tool is active.
// Switching through NONE tears down editable page state and can wait forever
// for rerenders on PDFs that already contain annotations.
const SELECT_BACKING_MODE = AnnotationEditorType.HIGHLIGHT

// Literal hex, never tokens: these values are handed to PDF.js and written into
// the saved PDF as the annotation's own color. A CSS variable would not resolve
// there, and even if it did, the mark's color must not change when the app's
// theme does — it belongs to the document from then on.
const HIGHLIGHT_COLORS = [
  { value: '#facc15', labelKey: 'annotationColorYellow' },
  { value: '#84cc16', labelKey: 'annotationColorGreen' },
  { value: '#ef4444', labelKey: 'annotationColorRed' },
]
const DRAWING_COLORS = [
  { value: '#dc2626', labelKey: 'annotationColorRed' },
  { value: '#2563eb', labelKey: 'annotationColorBlue' },
  { value: '#111827', labelKey: 'annotationColorBlack' },
]

const props = defineProps({
  document: {
    type: Object,
    required: true,
  },
  // Translation artifacts and previously exported translated PDFs use a direct
  // path. Source documents are read through their registry id instead.
  pdfPath: {
    type: String,
    default: '',
  },
  pdfPathKind: {
    type: String,
    default: 'artifact', // artifact | file
  },
  target: {
    type: String,
    default: 'source', // source | translation
  },
  activePage: {
    type: Number,
    default: 1,
  },
  ui: {
    type: Object,
    required: true,
  },
})

const emit = defineEmits([
  'loaded',
  'load-failed',
  'page-change',
  'state-change',
  'saved',
  'close',
])

const root = ref(null)
const container = ref(null)
const viewerElement = ref(null)
const pdfDocument = shallowRef(null)
const pdfViewer = shallowRef(null)
const editorManager = shallowRef(null)
const loading = ref(false)
const error = ref('')
const saving = ref(false)
const dirty = ref(false)
const activeTool = ref(SELECT_TOOL)
const editorReady = ref(false)
const currentPage = ref(1)
const pageCount = ref(0)
const scale = ref(DEFAULT_SCALE)
const canUndo = ref(false)
const canRedo = ref(false)
const canDelete = ref(false)
const savedPath = ref('')
const editRevision = ref(0)
const selectedColors = ref({
  highlight: HIGHLIGHT_COLORS[0].value,
  freetext: DRAWING_COLORS[0].value,
  ink: DRAWING_COLORS[0].value,
})
const customColors = ref({
  highlight: [],
  freetext: [],
  ink: [],
})
const customColorInput = ref(null)

let eventBus = null
let linkService = null
let loadTask = null
let loadRun = 0
let mounted = false
let shortcutInstalled = false
let desiredTool = SELECT_TOOL
let toolSyncTask = null
let cancelPendingToolSwitch = null
let customColorGroup = ''
let annotationStorage = null
let annotationModifiedHandler = null
let previousAnnotationModifiedHandler = null
let annotationDirtyTrackingReady = false
let pdfDestroyTask = Promise.resolve()
let savedAnnotationHash = ''
let observedAnnotationHash = ''
let saveRun = 0

const isTranslationTarget = computed(() => props.target === 'translation')
const saveLabel = computed(() => label('annotationSave', 'Save'))
const saveAsLabel = computed(() => label('annotationSaveAs', 'Save as'))
const exitLabel = computed(() => label('annotationExit', 'Exit annotation'))
const toolItems = computed(() => [
  { id: SELECT_TOOL, mode: SELECT_TOOL, label: label('annotationSelect', 'Select') },
  { id: 'highlight', mode: AnnotationEditorType.HIGHLIGHT, label: label('annotationHighlight', 'Highlight') },
  { id: 'text', mode: AnnotationEditorType.FREETEXT, label: label('annotationText', 'Text') },
  { id: 'ink', mode: AnnotationEditorType.INK, label: label('annotationDraw', 'Draw') },
])
const activeColorGroup = computed(() => {
  if (activeTool.value === AnnotationEditorType.HIGHLIGHT) return 'highlight'
  if (activeTool.value === AnnotationEditorType.FREETEXT) return 'freetext'
  if (activeTool.value === AnnotationEditorType.INK) return 'ink'
  return ''
})
const activeColorPalette = computed(() => {
  const group = activeColorGroup.value
  if (!group) return []
  const defaults = group === 'highlight' ? HIGHLIGHT_COLORS : DRAWING_COLORS
  return [
    ...defaults,
    ...customColors.value[group].map((value) => ({ value, labelKey: 'annotationCustomColor' })),
  ]
})
const activeColor = computed(() => (
  activeColorGroup.value ? selectedColors.value[activeColorGroup.value] : ''
))
const toolbarStatus = computed(() => {
  if (saving.value) return label('annotationSaving', 'Saving…')
  if (error.value) return error.value
  if (dirty.value) return label('annotationUnsaved', 'Unsaved changes')
  return label('annotationSaved', 'Saved')
})

function label(key, fallback) {
  return props.ui?.[key] || fallback
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, Number(value) || min))
}

function installShortcutHandler() {
  if (shortcutInstalled) return
  shortcutInstalled = true
  window.addEventListener('keydown', handleShortcut, true)
}

function removeShortcutHandler() {
  if (!shortcutInstalled) return
  shortcutInstalled = false
  window.removeEventListener('keydown', handleShortcut, true)
}

function handleShortcut(event) {
  const target = event.target instanceof Element ? event.target : null
  if (!target || !root.value?.contains(target)) return
  if (saving.value) {
    event.preventDefault()
    event.stopImmediatePropagation()
    return
  }
  if (event.defaultPrevented) return
  if (target.closest('input, textarea, [contenteditable="true"]')) return
  // PDF.js owns shortcuts while a drawing/text tool is active. Its Select mode
  // intentionally disables that listener, so provide the same commands there.
  if (activeTool.value !== SELECT_TOOL || container.value?.contains(target)) return

  const modifier = event.ctrlKey || event.metaKey
  if (!modifier) return
  const key = event.key.toLowerCase()
  if (key === 'z') {
    event.preventDefault()
    if (event.shiftKey) redo()
    else undo()
  } else if (key === 'y') {
    event.preventDefault()
    redo()
  }
}

async function loadPdf() {
  if (!mounted || !container.value || !viewerElement.value) return
  const run = ++loadRun
  saving.value = false
  await destroyPdf()
  if (run !== loadRun) return
  loading.value = true
  error.value = ''
  dirty.value = false
  savedPath.value = isTranslationTarget.value && props.pdfPathKind === 'file' ? props.pdfPath : ''
  canUndo.value = false
  canRedo.value = false
  canDelete.value = false
  editRevision.value = 0
  savedAnnotationHash = ''
  observedAnnotationHash = ''
  currentPage.value = 1
  pageCount.value = 0
  activeTool.value = SELECT_TOOL
  editorReady.value = false
  annotationDirtyTrackingReady = false
  desiredTool = SELECT_TOOL

  try {
    const bytes = await readPdfBytes()
    if (run !== loadRun) return

    eventBus = new EventBus()
    linkService = new PDFLinkService({ eventBus })
    eventBus.on('annotationeditoruimanager', ({ uiManager }) => {
      if (run !== loadRun) return
      editorManager.value = uiManager
      installShortcutHandler()
    })
    eventBus.on('annotationeditorlayerrendered', () => {
      if (run !== loadRun || annotationDirtyTrackingReady) return
      queueMicrotask(() => {
        if (run !== loadRun || annotationDirtyTrackingReady) return
        // Existing editors hydrate annotationStorage when their page becomes
        // visible. Treat the first editor layer as the initial saved baseline.
        pdfDocument.value?.annotationStorage?.resetModified?.()
        savedAnnotationHash = annotationHash(pdfDocument.value)
        observedAnnotationHash = savedAnnotationHash
        dirty.value = false
        annotationDirtyTrackingReady = true
      })
    })
    eventBus.on('annotationeditormodechanged', ({ mode }) => {
      if (run !== loadRun) return
      editorReady.value = true
      if (editorModeForTool(desiredTool) === mode) {
        activeTool.value = desiredTool
        error.value = ''
        applyDefaultColorForTool(desiredTool, mode, editorManager.value)
      }

    })
    eventBus.on('editingstateschanged', ({ details }) => {
      if (run !== loadRun) return
      canUndo.value = Boolean(details?.hasSomethingToUndo)
      canRedo.value = Boolean(details?.hasSomethingToRedo)
      canDelete.value = Boolean(details?.hasSelectedEditor)
      queueMicrotask(() => updateDirtyFromStorage(run))
      emitViewerState()
    })
    eventBus.on('pagechanging', ({ pageNumber }) => {
      if (run !== loadRun) return
      currentPage.value = Number(pageNumber) || 1
      emit('page-change', currentPage.value)
      emitViewerState()
    })
    eventBus.on('scalechanging', ({ scale: nextScale }) => {
      if (run !== loadRun) return
      scale.value = Number(nextScale) || scale.value
      emitViewerState()
    })

    const task = pdfjsLib.getDocument({
      data: bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes),
      cMapUrl: '/pdfjs/cmaps/',
      cMapPacked: true,
      standardFontDataUrl: '/pdfjs/standard_fonts/',
    })
    loadTask = task
    const document = await task.promise
    if (run !== loadRun) {
      try {
        await document.destroy?.()
      } catch {
        // A newer load already owns the viewer and worker lifecycle.
      }
      return
    }

    const viewer = new PDFViewer({
      container: container.value,
      viewer: viewerElement.value,
      eventBus,
      linkService,
      annotationEditorMode: SELECT_BACKING_MODE,
      annotationMode: pdfjsLib.AnnotationMode.ENABLE_FORMS,
    })
    pdfDocument.value = document
    bindAnnotationStorage(document, run)
    pdfViewer.value = viewer
    linkService.setViewer(viewer)
    linkService.setDocument(document)
    viewer.currentScale = scale.value
    viewer.setDocument(document)

    pageCount.value = document.numPages
    const initialPage = clamp(props.activePage, 1, pageCount.value || 1)
    await nextTick()
    if (run !== loadRun) return
    goToPage(initialPage, { behavior: 'auto', emitPageChange: false })
    emit('loaded', { pageCount: pageCount.value })
    emitViewerState()
  } catch (err) {
    if (run !== loadRun) return
    error.value = err?.message || String(err)
    emit('load-failed', { error: error.value })
    emitViewerState()
  } finally {
    if (run === loadRun) loading.value = false
  }
}

async function readPdfBytes() {
  if (!props.pdfPath) {
    return invoke('read_pdf_bytes', { docId: props.document.id })
  }
  const command = props.pdfPathKind === 'file'
    ? 'read_saved_pdf_bytes'
    : 'read_pdf_artifact_bytes'
  return invoke(command, { path: props.pdfPath })
}

function destroyPdf() {
  saveRun += 1
  cancelPendingToolSwitch?.()
  cancelPendingToolSwitch = null
  toolSyncTask = null
  unbindAnnotationStorage()
  annotationDirtyTrackingReady = false
  removeShortcutHandler()
  editorReady.value = false
  editorManager.value = null
  eventBus = null
  linkService = null
  const task = loadTask
  const document = pdfDocument.value
  loadTask = null
  pdfViewer.value?.cleanup?.()
  pdfViewer.value?.setDocument?.(null)
  pdfViewer.value = null
  pdfDocument.value = null
  if (viewerElement.value) viewerElement.value.replaceChildren()
  const previousDestroy = pdfDestroyTask
  pdfDestroyTask = (async () => {
    await previousDestroy.catch(() => {})
    try {
      if (task?.destroy) await task.destroy()
      else await document?.destroy?.()
    } catch {
      // A cancelled loading task can reject while it is already terminating.
    }
  })()
  return pdfDestroyTask
}

function bindAnnotationStorage(document, run) {
  unbindAnnotationStorage()
  const storage = document?.annotationStorage
  if (!storage) return
  const previous = storage.onSetModified
  const onModified = () => {
    previous?.call(storage)
    if (run !== loadRun || !annotationDirtyTrackingReady) return
    queueMicrotask(() => updateDirtyFromStorage(run))
  }
  annotationStorage = storage
  annotationModifiedHandler = onModified
  previousAnnotationModifiedHandler = previous
  storage.onSetModified = onModified
}

function unbindAnnotationStorage() {
  if (annotationStorage?.onSetModified === annotationModifiedHandler) {
    annotationStorage.onSetModified = previousAnnotationModifiedHandler
  }
  annotationStorage = null
  annotationModifiedHandler = null
  previousAnnotationModifiedHandler = null
}

function annotationHash(document = pdfDocument.value) {
  return document?.annotationStorage?.serializable?.hash || ''
}

function updateDirtyFromStorage(run = loadRun) {
  if (run !== loadRun || !annotationDirtyTrackingReady || !pdfDocument.value) return
  const hash = annotationHash(pdfDocument.value)
  if (hash !== observedAnnotationHash) {
    observedAnnotationHash = hash
    editRevision.value += 1
  }
  dirty.value = hash !== savedAnnotationHash
  emitViewerState()
}

function setTool(mode) {
  if (saving.value) return Promise.resolve(false)
  desiredTool = mode
  error.value = ''
  if (!editorReady.value || !editorManager.value || !pdfViewer.value) return Promise.resolve(false)
  if (toolSyncTask) return toolSyncTask
  const task = synchronizeRequestedTool()
  toolSyncTask = task
  task.finally(() => {
    if (toolSyncTask === task) toolSyncTask = null
  })
  return task
}

async function synchronizeRequestedTool() {
  while (mounted && editorReady.value && pdfViewer.value && editorManager.value) {
    const viewer = pdfViewer.value
    const manager = editorManager.value
    const bus = eventBus
    const requestedTool = desiredTool
    const mode = editorModeForTool(requestedTool)
    if (viewer.annotationEditorMode === mode && manager.getMode?.() === mode) {
      activeTool.value = requestedTool
      applyDefaultColorForTool(requestedTool, mode, manager)
      if (requestedTool === desiredTool) return
      continue
    }
    const changed = await switchAnnotationEditorMode(viewer, manager, bus, mode)
    if (!changed && requestedTool === desiredTool) break
  }
}

function editorModeForTool(tool) {
  return tool === SELECT_TOOL ? SELECT_BACKING_MODE : tool
}

function switchAnnotationEditorMode(viewer, manager, bus, mode) {
  return new Promise((resolve) => {
    let settled = false
    const complete = (applied) => {
      if (settled) return
      settled = true
      window.clearTimeout(timeoutId)
      bus?.off('annotationeditormodechanged', onModeChanged)
      if (cancelPendingToolSwitch === cancel) cancelPendingToolSwitch = null
      if (applied) {
        if (editorModeForTool(desiredTool) === mode) {
          activeTool.value = desiredTool
          applyDefaultColorForTool(desiredTool, mode, manager)
        }
      }
      resolve(applied)
    }
    const onModeChanged = ({ mode: changedMode }) => {
      if (changedMode === mode && pdfViewer.value === viewer && editorManager.value === manager) complete(true)
    }
    const cancel = () => complete(false)
    const timeoutId = window.setTimeout(() => {
      if (pdfViewer.value === viewer && editorManager.value === manager) {
        const viewerMode = viewer.annotationEditorMode
        const managerMode = manager.getMode?.()
        if (viewerMode === mode && managerMode === mode) {
          complete(true)
          return
        }
        error.value = label('annotationToolSwitchFailed', 'Could not switch annotation tool')
      }
      complete(false)
    }, 8000)

    cancelPendingToolSwitch = cancel
    bus?.on('annotationeditormodechanged', onModeChanged)
    try {
      // PDFViewer owns both its private mode and the UI manager. Bypassing this
      // setter can leave them disagreeing, which breaks every later switch.
      if (viewer.annotationEditorMode !== mode) {
        viewer.annotationEditorMode = { mode }
      } else if (manager.getMode?.() === mode) {
        complete(true)
      }
    } catch (err) {
      error.value = err?.message || String(err)
      complete(false)
    }
  })
}

function colorParamForMode(mode) {
  if (mode === AnnotationEditorType.HIGHLIGHT) return AnnotationEditorParamsType.HIGHLIGHT_COLOR
  if (mode === AnnotationEditorType.FREETEXT) return AnnotationEditorParamsType.FREETEXT_COLOR
  if (mode === AnnotationEditorType.INK) return AnnotationEditorParamsType.INK_COLOR
  return null
}

function applyDefaultColorForTool(tool, mode, manager = editorManager.value) {
  if (tool === SELECT_TOOL) return
  if (tool === AnnotationEditorType.HIGHLIGHT) manager?.unselectAll?.()
  const param = colorParamForMode(mode)
  if (!param) return
  const group = mode === AnnotationEditorType.HIGHLIGHT
    ? 'highlight'
    : mode === AnnotationEditorType.FREETEXT ? 'freetext' : 'ink'
  manager?.updateParams?.(param, selectedColors.value[group])
}

function setColor(color) {
  if (saving.value) return
  const group = activeColorGroup.value
  const param = colorParamForMode(activeTool.value)
  if (!group || !param) return
  selectedColors.value[group] = color
  editorManager.value?.updateParams?.(param, color)
}

function openCustomColorPicker() {
  if (saving.value) return
  if (!activeColorGroup.value) return
  customColorGroup = activeColorGroup.value
  customColorInput.value?.click()
}

function addCustomColor(event) {
  if (saving.value) return
  const group = customColorGroup
  const color = String(event.target?.value || '').trim().toLowerCase()
  if (!group || !/^#[0-9a-f]{6}$/.test(color)) return
  const exists = activeColorPalette.value.some((item) => item.value.toLowerCase() === color)
  if (!exists) customColors.value[group].push(color)
  setColor(color)
}

function preventInkContextMenu(event) {
  if (activeTool.value === AnnotationEditorType.INK) event.preventDefault()
}

function markEditorInput() {
  if (saving.value || !annotationDirtyTrackingReady) return
  editRevision.value += 1
  dirty.value = true
  emitViewerState()
}

function undo() {
  if (saving.value || !canUndo.value) return
  editorManager.value?.undo?.()
}

function redo() {
  if (saving.value || !canRedo.value) return
  editorManager.value?.redo?.()
}

function eraseSelection() {
  if (saving.value || !canDelete.value) return
  editorManager.value?.delete?.()
}

async function save({ saveAs = false } = {}) {
  const document = pdfDocument.value
  if (!document || saving.value) return
  const run = loadRun
  const operation = ++saveRun
  const documentId = props.document.id
  const documentTitle = props.document?.title || props.document?.shortTitle || 'document.pdf'
  const target = props.target
  const translationTarget = target === 'translation'
  const outputPath = savedPath.value
  const manager = editorManager.value
  saving.value = true
  error.value = ''
  const isCurrentSave = () => operation === saveRun
    && run === loadRun
    && pdfDocument.value === document
  try {
    // Commit contenteditable FreeText and an in-progress drawing before PDF.js
    // serializes annotationStorage. Otherwise Save can omit the last action.
    manager?.commitOrRemove?.()
    manager?.currentLayer?.endDrawingSession?.(false)
    await nextTick()
    if (!isCurrentSave()) return
    updateDirtyFromStorage(run)
    const savedRevision = editRevision.value
    const savedHash = annotationHash(document)
    const bytes = await document.saveDocument()
    if (!isCurrentSave()) return
    let result
    if (translationTarget) {
      if (saveAs || !outputPath) {
        result = await invoke('save_pdf_as', {
          input: { defaultName: defaultFileName(documentTitle, true), bytes },
        })
      } else {
        result = await invoke('save_pdf_at_path', {
          input: { path: outputPath, bytes },
        })
      }
    } else if (saveAs) {
      result = await invoke('save_pdf_document_as', {
        input: { documentId, defaultName: defaultFileName(documentTitle, false), bytes },
      })
    } else {
      result = await invoke('save_pdf_document', {
        input: { documentId, bytes },
      })
    }

    if (!result || !isCurrentSave()) return
    savedPath.value = result.path || outputPath
    if (editRevision.value === savedRevision && annotationHash(document) === savedHash) {
      savedAnnotationHash = savedHash
      observedAnnotationHash = savedHash
      dirty.value = false
    } else {
      updateDirtyFromStorage(run)
    }
    emit('saved', {
      target,
      path: result.path || '',
      size: Number(result.size || 0),
      pathKind: translationTarget ? 'file' : 'source',
    })
    emitViewerState()
  } catch (err) {
    if (!isCurrentSave()) return
    error.value = err?.message || String(err)
    emitViewerState()
  } finally {
    if (operation === saveRun) saving.value = false
  }
}

function defaultFileName(title = props.document?.title || props.document?.shortTitle || 'document.pdf', translationTarget = isTranslationTarget.value) {
  const source = String(title)
  const stem = source.replace(/\.pdf$/i, '').trim() || 'document'
  return translationTarget
    ? `${stem}.translated.annotated.pdf`
    : `${stem}.annotated.pdf`
}

function requestClose() {
  if (dirty.value && !window.confirm(label('annotationDiscardConfirm', 'Discard unsaved PDF annotations?'))) {
    return
  }
  emit('close', { target: props.target, path: savedPath.value })
}

function goToPage(page, options = {}) {
  const viewer = pdfViewer.value
  if (!viewer || !pageCount.value) return
  const target = clamp(page, 1, pageCount.value)
  viewer.currentPageNumber = target
  viewer.scrollPageIntoView?.({ pageNumber: target })
  currentPage.value = target
  if (options.emitPageChange !== false) emit('page-change', target)
  emitViewerState()
}

function setScale(nextScale) {
  const viewer = pdfViewer.value
  if (!viewer) return
  const next = clamp(nextScale, MIN_SCALE, MAX_SCALE)
  viewer.currentScale = next
  scale.value = next
  emitViewerState()
}

function currentScrollAnchor() {
  const viewer = pdfViewer.value
  const scroll = container.value
  if (!viewer || !scroll) return null
  const page = viewer.currentPageNumber || currentPage.value
  const pageView = viewer.getPageView?.(page - 1)
  if (!pageView?.div?.clientHeight) return { page, progress: 0 }
  const progress = clamp(
    (scroll.scrollTop - pageView.div.offsetTop) / pageView.div.clientHeight,
    0,
    1,
  )
  return { page, progress }
}

function scrollToPageProgress(page, progress = 0, options = {}) {
  const viewer = pdfViewer.value
  const scroll = container.value
  if (!viewer || !scroll) return
  const target = clamp(page, 1, pageCount.value || 1)
  const pageView = viewer.getPageView?.(target - 1)
  if (pageView?.div) {
    scroll.scrollTo({
      top: pageView.div.offsetTop + pageView.div.clientHeight * clamp(progress, 0, 1),
      behavior: options.behavior || 'auto',
    })
  } else {
    goToPage(target, { emitPageChange: options.emitPageChange })
  }
}

function visiblePages() {
  return pdfViewer.value?.getVisiblePages?.()?.views
    ?.map((item) => Number(item.id))
    .filter((page) => page > 0) || []
}

function emitViewerState() {
  emit('state-change', {
    currentPage: currentPage.value,
    pageCount: pageCount.value,
    visiblePages: visiblePages(),
    scale: scale.value,
    canGoPrevious: currentPage.value > 1,
    canGoNext: currentPage.value < pageCount.value,
    loading: loading.value,
    error: Boolean(error.value),
  })
}

watch(
  [() => props.document?.id, () => props.pdfPath, () => props.pdfPathKind, () => props.target],
  () => {
    if (mounted) loadPdf()
  },
)

watch(() => props.activePage, (page) => {
  if (!pdfViewer.value || !page || page === currentPage.value) return
  goToPage(page, { emitPageChange: false })
})

onMounted(async () => {
  mounted = true
  await nextTick()
  loadPdf()
})

onActivated(() => {
  nextTick(() => {
    if (!pdfViewer.value) return
    pdfViewer.value.currentScale = scale.value
    pdfViewer.value.refresh()
    emitViewerState()
  })
})

onBeforeUnmount(() => {
  mounted = false
  loadRun += 1
  destroyPdf()
})

defineExpose({
  goToPage,
  setScale,
  currentScrollAnchor,
  scrollToPageProgress,
})
</script>

<template>
  <section ref="root" class="pdf-annotation-viewer" :class="{ 'is-saving': saving }" :aria-busy="saving" :aria-label="label('annotationWorkspace', 'PDF annotation editor')">
    <header class="annotation-toolbar">
      <div class="annotation-tools" role="toolbar" :aria-label="label('annotationTools', 'Annotation tools')">
        <button
          v-for="tool in toolItems"
          :key="tool.id"
          type="button"
          class="annotation-tool icon-only"
          :class="{ active: activeTool === tool.mode }"
          :title="tool.label"
          :aria-label="tool.label"
          :aria-pressed="activeTool === tool.mode"
          :disabled="!editorReady || saving"
          @click="setTool(tool.mode)"
        >
          <svg v-if="tool.id === 'select'" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M5 3.5 18.2 12 12 13.8l-2.8 6.7L5 3.5Z" />
            <path d="m12.4 13.7 4.2 5" />
          </svg>
          <svg v-else-if="tool.id === 'highlight'" viewBox="0 0 24 24" aria-hidden="true">
            <path d="m5 15 8.8-8.8 4 4L9 19H5v-4Z" />
            <path d="m12.3 6.5 4 4" />
            <path d="M4 21h16" />
          </svg>
          <svg v-else-if="tool.id === 'text'" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M5 5h14" />
            <path d="M12 5v14" />
            <path d="M8.5 19h7" />
          </svg>
          <svg v-else viewBox="0 0 24 24" aria-hidden="true">
            <path d="m4 17 10.8-10.8 4 4L8 21H4v-4Z" />
            <path d="m13.5 6.3 4 4" />
            <path d="M3 21c3.8-1.1 7.6-1.1 11.4 0" />
          </svg>
          <span class="sr-only">{{ tool.label }}</span>
        </button>
        <span class="annotation-divider" aria-hidden="true"></span>
        <button
          type="button"
          class="annotation-tool icon-only danger"
          :disabled="saving || !canDelete"
          :title="label('annotationErase', 'Delete selected annotation')"
          :aria-label="label('annotationErase', 'Delete selected annotation')"
          @click="eraseSelection"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="m7 15 7.5-7.5 5 5L12 20H7v-5Z" />
            <path d="m4 20h16" />
          </svg>
          <span class="sr-only">{{ label('annotationErase', 'Erase') }}</span>
        </button>
        <button
          type="button"
          class="annotation-tool icon-only"
          :disabled="saving || !canUndo"
          :title="label('annotationUndo', 'Undo')"
          :aria-label="label('annotationUndo', 'Undo')"
          @click="undo"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="m9 8-5 4 5 4" />
            <path d="M5 12h9a5 5 0 0 1 5 5v1" />
          </svg>
          <span class="sr-only">{{ label('annotationUndo', 'Undo') }}</span>
        </button>
        <button
          type="button"
          class="annotation-tool icon-only"
          :disabled="saving || !canRedo"
          :title="label('annotationRedo', 'Redo')"
          :aria-label="label('annotationRedo', 'Redo')"
          @click="redo"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="m15 8 5 4-5 4" />
            <path d="M19 12h-9a5 5 0 0 0-5 5v1" />
          </svg>
          <span class="sr-only">{{ label('annotationRedo', 'Redo') }}</span>
        </button>
      </div>
      <div class="annotation-actions">
        <div v-if="activeColorPalette.length" class="annotation-color-picker" role="radiogroup" :aria-label="label('annotationColor', 'Annotation color')">
          <button
            v-for="color in activeColorPalette"
            :key="color.value"
            type="button"
            class="annotation-color"
            :class="{ active: activeColor === color.value }"
            :style="{ '--annotation-color': color.value }"
            :title="label(color.labelKey, color.value)"
            :aria-label="label(color.labelKey, color.value)"
            :aria-checked="activeColor === color.value"
            :disabled="saving"
            role="radio"
            @click="setColor(color.value)"
          ><span aria-hidden="true"></span></button>
          <button
            type="button"
            class="annotation-color annotation-color-add"
            :title="label('annotationAddColor', 'Add color')"
            :aria-label="label('annotationAddColor', 'Add color')"
            :disabled="saving"
            @click="openCustomColorPicker"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
            <span class="sr-only">{{ label('annotationAddColor', 'Add color') }}</span>
          </button>
          <input ref="customColorInput" class="native-color-input" type="color" value="#2563eb" tabindex="-1" @change="addCustomColor" />
        </div>
        <span class="annotation-status" :class="{ error: error, dirty }">{{ toolbarStatus }}</span>
        <button type="button" class="annotation-action" :disabled="saving || loading" @click="save()">{{ saveLabel }}</button>
        <button type="button" class="annotation-action" :disabled="saving || loading" @click="save({ saveAs: true })">{{ saveAsLabel }}</button>
        <button type="button" class="annotation-action quiet" :disabled="saving" @click="requestClose">{{ exitLabel }}</button>
      </div>
    </header>
    <div v-if="loading" class="annotation-placeholder">{{ label('pdfLoading', 'Loading PDF...') }}</div>
    <div v-else-if="error && !pdfDocument" class="annotation-placeholder error">{{ label('pdfLoadFailed', 'PDF failed to load') }}: {{ error }}</div>
    <div
      ref="container"
      class="annotation-pdf-container"
      :class="{
        hidden: loading || (error && !pdfDocument),
        'is-select': activeTool === SELECT_TOOL,
        'is-highlight': activeTool === AnnotationEditorType.HIGHLIGHT,
        'is-ink': activeTool === AnnotationEditorType.INK,
      }"
      :inert="saving"
      @input.capture="markEditorInput"
      @contextmenu.capture="preventInkContextMenu"
    >
      <div ref="viewerElement" class="pdfViewer"></div>
    </div>
  </section>
</template>

<style scoped>
.pdf-annotation-viewer {
  position: relative;
  display: flex;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  flex-direction: column;
  overflow: hidden;
  background: var(--surface-1);
}

.annotation-toolbar {
  z-index: 12;
  display: flex;
  min-height: 42px;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 5px 8px;
  border-bottom: 1px solid var(--line);
  background: var(--surface-1);
}

.annotation-tools,
.annotation-actions {
  display: inline-flex;
  min-width: 0;
  align-items: center;
  gap: 4px;
}

.annotation-actions {
  flex-shrink: 0;
}

.annotation-tool,
.annotation-action {
  min-height: 28px;
  border: 1px solid transparent;
  border-radius: var(--r-sm);
  padding: 0 8px;
  background: transparent;
  color: var(--ink-2);
  cursor: pointer;
  font-size: 11px;
  white-space: nowrap;
}

.annotation-tool.icon-only {
  display: inline-grid;
  width: 30px;
  min-width: 30px;
  place-items: center;
  padding: 0;
}

.annotation-tool.icon-only svg {
  width: 17px;
  height: 17px;
  fill: none;
  stroke: currentColor;
  stroke-linecap: round;
  stroke-linejoin: round;
  stroke-width: 1.8;
}

.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
}

.annotation-tool:hover:not(:disabled),
.annotation-action:hover:not(:disabled) {
  border-color: var(--accent-line);
  background: var(--accent-tint);
  color: var(--ink);
}

.annotation-tool.active,
.annotation-action:not(.quiet) {
  border-color: var(--accent-line);
  background: var(--accent-tint);
  color: var(--accent-ink);
}

.annotation-tool.danger:hover:not(:disabled) {
  border-color: var(--danger-line);
  background: var(--danger-tint);
  color: var(--danger);
}

.annotation-tool:disabled,
.annotation-action:disabled {
  cursor: not-allowed;
  opacity: 0.4;
}

.annotation-divider {
  width: 1px;
  height: 20px;
  margin: 0 2px;
  background: var(--line);
}

.annotation-color-picker {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding-right: 3px;
}

.annotation-color {
  display: grid;
  width: 22px;
  height: 22px;
  place-items: center;
  border: 1px solid transparent;
  border-radius: 50%;
  padding: 0;
  background: transparent;
  cursor: pointer;
}

/* Deliberately raw, not tokenized: these two rings sit directly on the user's
   own highlight color, which can be anything they pick. They need a fixed
   dark/light pair to stay legible against a yellow swatch and a black one
   alike — a theme token would follow the app and lose that guarantee. */
.annotation-color span {
  width: 14px;
  height: 14px;
  border: 1px solid var(--line-strong);
  border-radius: 50%;
  background: var(--annotation-color);
  box-shadow: 0 0 0 1px rgba(15, 23, 42, 0.45);
}

.annotation-color:hover,
.annotation-color.active {
  border-color: var(--line-strong);
  background: var(--line);
}

.annotation-color:disabled {
  cursor: not-allowed;
  opacity: 0.4;
}

.annotation-color.active span {
  box-shadow: 0 0 0 2px #1e293b, 0 0 0 3px #e2e8f0;
}

.annotation-color-add {
  color: var(--ink-2);
}

.annotation-color-add svg {
  width: 15px;
  height: 15px;
  fill: none;
  stroke: currentColor;
  stroke-linecap: round;
  stroke-width: 2;
}

.native-color-input {
  position: absolute;
  width: 1px;
  height: 1px;
  opacity: 0;
  pointer-events: none;
}

.annotation-status {
  max-width: 150px;
  overflow: hidden;
  color: var(--ink-3);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.annotation-status.dirty { color: var(--warning); }
.annotation-status.error { color: var(--danger); }

.annotation-pdf-container {
  position: absolute;
  inset: 42px 0 0;
  overflow: auto;
  background: var(--surface-3);
}

/* Highlight and Ink must draw through existing annotations. FreeText keeps
   PDF.js' native hit-testing so an existing text box can be edited again. */
.annotation-pdf-container.is-highlight :deep(.annotationEditorLayer > :is(.freeTextEditor, .inkEditor, .highlightEditor, .stampEditor, .signatureEditor)),
.annotation-pdf-container.is-ink :deep(.annotationEditorLayer > :is(.freeTextEditor, .inkEditor, .highlightEditor, .stampEditor, .signatureEditor)) {
  pointer-events: none !important;
}

.annotation-pdf-container.is-highlight :deep(.annotationEditorLayer > :is(.freeTextEditor, .inkEditor, .highlightEditor, .stampEditor, .signatureEditor) *),
.annotation-pdf-container.is-ink :deep(.annotationEditorLayer > :is(.freeTextEditor, .inkEditor, .highlightEditor, .stampEditor, .signatureEditor) *) {
  pointer-events: none !important;
}

.annotation-pdf-container.is-highlight :deep(.annotationEditorLayer .editToolbar),
.annotation-pdf-container.is-ink :deep(.annotationEditorLayer .editToolbar) {
  pointer-events: none !important;
}

/* Select is a logical tool backed by PDF.js Highlight mode. Block the text
   layer so dragging does not create highlights, while retaining hit-testing on
   every existing editor and its toolbar. */
.annotation-pdf-container.is-select :deep(.textLayer) {
  pointer-events: none !important;
  user-select: none !important;
}

.annotation-pdf-container.is-select :deep(.annotationEditorLayer) {
  pointer-events: none !important;
}

.annotation-pdf-container.is-select :deep(.annotationEditorLayer > :is(.freeTextEditor, .inkEditor, .highlightEditor, .stampEditor, .signatureEditor)),
.annotation-pdf-container.is-select :deep(.annotationEditorLayer > :is(.freeTextEditor, .inkEditor, .highlightEditor, .stampEditor, .signatureEditor) .internal),
.annotation-pdf-container.is-select :deep(.annotationEditorLayer .editToolbar) {
  pointer-events: auto !important;
}

/* Let PDF.js receive the complete pointer stream. WebView otherwise treats
   touch/pen movement as pan/long-press gestures before Ink can consume it. */
.annotation-pdf-container.is-ink,
.annotation-pdf-container.is-ink :deep(.annotationEditorLayer),
.annotation-pdf-container.is-ink :deep(.annotationEditorLayer *) {
  touch-action: none !important;
  overscroll-behavior: contain;
}

.annotation-pdf-container.is-ink :deep(.annotationEditorLayer) {
  cursor: crosshair;
}

.annotation-pdf-container.hidden {
  pointer-events: none;
  visibility: hidden;
}

.annotation-pdf-container :deep(.pdfViewer) {
  --scale-factor: 1.18;
  padding: 16px 0 28px;
}

.annotation-placeholder {
  display: grid;
  flex: 1 1 auto;
  place-items: center;
  padding: 24px;
  color: var(--ink-3);
  font-size: 13px;
  text-align: center;
}

.annotation-placeholder.error { color: var(--danger); }

@media (max-width: 960px) {
  .annotation-toolbar {
    align-items: flex-start;
    flex-direction: column;
  }

  .annotation-actions {
    width: 100%;
  }

  .annotation-status {
    margin-right: auto;
  }

  .annotation-pdf-container { inset: 76px 0 0; }
}
</style>
