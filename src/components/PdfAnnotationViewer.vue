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
const activeTool = ref(AnnotationEditorType.NONE)
const currentPage = ref(1)
const pageCount = ref(0)
const scale = ref(DEFAULT_SCALE)
const canUndo = ref(false)
const canRedo = ref(false)
const canDelete = ref(false)
const savedPath = ref('')
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
let desiredTool = AnnotationEditorType.NONE
let toolSyncTask = null
let customColorGroup = ''

const isTranslationTarget = computed(() => props.target === 'translation')
const saveLabel = computed(() => label('annotationSave', 'Save'))
const saveAsLabel = computed(() => label('annotationSaveAs', 'Save as'))
const exitLabel = computed(() => label('annotationExit', 'Exit annotation'))
const toolItems = computed(() => [
  { id: 'select', mode: AnnotationEditorType.NONE, label: label('annotationSelect', 'Select') },
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
  window.addEventListener('keydown', handleShortcut)
}

function removeShortcutHandler() {
  if (!shortcutInstalled) return
  shortcutInstalled = false
  window.removeEventListener('keydown', handleShortcut)
}

function handleShortcut(event) {
  const target = event.target instanceof Element ? event.target : null
  if (!target || !root.value?.contains(target) || event.defaultPrevented) return
  if (target.closest('input, textarea, [contenteditable="true"]')) return
  // PDF.js owns shortcuts while a drawing/text tool is active. Its Select mode
  // intentionally disables that listener, so provide the same commands there.
  if (activeTool.value !== AnnotationEditorType.NONE) return

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
  destroyPdf()
  loading.value = true
  error.value = ''
  dirty.value = false
  canUndo.value = false
  canRedo.value = false
  canDelete.value = false
  currentPage.value = 1
  pageCount.value = 0
  activeTool.value = AnnotationEditorType.NONE
  desiredTool = AnnotationEditorType.NONE

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
    eventBus.on('annotationeditormodechanged', ({ mode }) => {
      if (run !== loadRun) return
      activeTool.value = mode
      applyDefaultColor(mode)
    })
    eventBus.on('editingstateschanged', ({ details }) => {
      if (run !== loadRun) return
      canUndo.value = Boolean(details?.hasSomethingToUndo)
      canRedo.value = Boolean(details?.hasSomethingToRedo)
      canDelete.value = Boolean(details?.hasSelectedEditor)
      // Initial editor setup reports an empty state. All later command-state
      // changes represent a user edit, undo, redo, or selection transition.
      if (details?.hasSomethingToUndo || details?.hasSomethingToRedo) dirty.value = true
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
      document.destroy?.()
      return
    }

    const viewer = new PDFViewer({
      container: container.value,
      viewer: viewerElement.value,
      eventBus,
      linkService,
      annotationEditorMode: AnnotationEditorType.NONE,
      annotationMode: pdfjsLib.AnnotationMode.ENABLE_FORMS,
    })
    pdfDocument.value = document
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
  removeShortcutHandler()
  editorManager.value = null
  eventBus = null
  linkService = null
  loadTask?.destroy?.()
  loadTask = null
  pdfViewer.value?.cleanup?.()
  pdfViewer.value?.setDocument?.(null)
  pdfViewer.value = null
  pdfDocument.value?.destroy?.()
  pdfDocument.value = null
  if (viewerElement.value) viewerElement.value.replaceChildren()
}

function setTool(mode) {
  desiredTool = mode
  error.value = ''
  if (toolSyncTask) return toolSyncTask
  toolSyncTask = synchronizeRequestedTool().finally(() => {
    toolSyncTask = null
    if (mounted && pdfViewer.value && activeTool.value !== desiredTool) {
      void setTool(desiredTool)
    }
  })
  return toolSyncTask
}

async function synchronizeRequestedTool() {
  while (mounted && pdfViewer.value && activeTool.value !== desiredTool) {
    const viewer = pdfViewer.value
    const mode = desiredTool
    const enteringEditMode = viewer.annotationEditorMode === AnnotationEditorType.NONE
      && mode !== AnnotationEditorType.NONE
    const changed = await switchAnnotationEditorMode(viewer, mode, enteringEditMode)
    if (!changed) break
  }
}

function switchAnnotationEditorMode(viewer, mode, enteringEditMode = false) {
  if (viewer.annotationEditorMode === mode) {
    activeTool.value = mode
    applyDefaultColor(mode)
    return Promise.resolve(true)
  }

  return new Promise((resolve) => {
    let settled = false
    const complete = (applied) => {
      if (settled) return
      settled = true
      window.clearTimeout(timeoutId)
      eventBus?.off('annotationeditormodechanged', onModeChanged)
      if (applied) {
        activeTool.value = mode
        applyDefaultColor(mode)
      }
      resolve(applied)
    }
    const onModeChanged = ({ mode: changedMode }) => {
      if (changedMode === mode) complete(true)
    }
    const timeoutId = window.setTimeout(async () => {
      // A page can be removed by PDF.js virtualization between the editing-mode
      // refresh and its pagerendered notification. Drive the public UI manager
      // directly as a recovery path instead of leaving its old tool active.
      try {
        await editorManager.value?.updateMode?.(mode, null, true)
        complete(true)
      } catch (err) {
        error.value = err?.message || String(err)
        complete(false)
      }
    }, 1400)

    eventBus?.on('annotationeditormodechanged', onModeChanged)
    try {
      viewer.annotationEditorMode = { mode }
      // PDF.js defers entering edit mode until every visible editable page has
      // rerendered. Refresh immediately so virtualized page views dispatch the
      // pagerendered events that complete that switch.
      if (enteringEditMode) viewer.refresh()
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

function applyDefaultColor(mode) {
  const param = colorParamForMode(mode)
  if (!param) return
  const group = mode === AnnotationEditorType.HIGHLIGHT
    ? 'highlight'
    : mode === AnnotationEditorType.FREETEXT ? 'freetext' : 'ink'
  editorManager.value?.updateParams?.(param, selectedColors.value[group])
}

function setColor(color) {
  const group = activeColorGroup.value
  const param = colorParamForMode(activeTool.value)
  if (!group || !param) return
  selectedColors.value[group] = color
  editorManager.value?.updateParams?.(param, color)
}

function openCustomColorPicker() {
  if (!activeColorGroup.value) return
  customColorGroup = activeColorGroup.value
  customColorInput.value?.click()
}

function addCustomColor(event) {
  const group = customColorGroup
  const color = String(event.target?.value || '').trim().toLowerCase()
  if (!group || !/^#[0-9a-f]{6}$/.test(color)) return
  const exists = activeColorPalette.value.some((item) => item.value.toLowerCase() === color)
  if (!exists) customColors.value[group].push(color)
  setColor(color)
}

function handleInkPointerDown(event) {
  if (activeTool.value !== AnnotationEditorType.INK) return
  if (event.pointerType !== 'pen' && event.pointerType !== 'touch') return
  event.preventDefault()
  const target = event.target
  target?.setPointerCapture?.(event.pointerId)
}

function preventInkContextMenu(event) {
  if (activeTool.value === AnnotationEditorType.INK) event.preventDefault()
}

function undo() {
  if (!canUndo.value) return
  editorManager.value?.undo?.()
}

function redo() {
  if (!canRedo.value) return
  editorManager.value?.redo?.()
}

function eraseSelection() {
  if (!canDelete.value) return
  editorManager.value?.delete?.()
}

async function save({ saveAs = false } = {}) {
  const document = pdfDocument.value
  if (!document || saving.value) return
  saving.value = true
  error.value = ''
  try {
    const bytes = await document.saveDocument()
    let result
    if (isTranslationTarget.value) {
      if (saveAs || !savedPath.value) {
        result = await invoke('save_pdf_as', {
          input: { defaultName: defaultFileName(), bytes },
        })
      } else {
        result = await invoke('save_pdf_at_path', {
          input: { path: savedPath.value, bytes },
        })
      }
    } else if (saveAs) {
      result = await invoke('save_pdf_document_as', {
        input: { documentId: props.document.id, defaultName: defaultFileName(), bytes },
      })
    } else {
      result = await invoke('save_pdf_document', {
        input: { documentId: props.document.id, bytes },
      })
    }

    if (!result) return
    savedPath.value = result.path || savedPath.value
    dirty.value = false
    emit('saved', {
      target: props.target,
      path: result.path || '',
      size: Number(result.size || 0),
      pathKind: isTranslationTarget.value ? 'file' : 'source',
    })
    emitViewerState()
  } catch (err) {
    error.value = err?.message || String(err)
    emitViewerState()
  } finally {
    saving.value = false
  }
}

function defaultFileName() {
  const source = String(props.document?.title || props.document?.shortTitle || 'document.pdf')
  const stem = source.replace(/\.pdf$/i, '').trim() || 'document'
  return isTranslationTarget.value
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
  [() => props.document?.id, () => props.pdfPath, () => props.pdfPathKind],
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
  <section ref="root" class="pdf-annotation-viewer" :aria-label="label('annotationWorkspace', 'PDF annotation editor')">
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
          :disabled="!canDelete"
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
          :disabled="!canUndo"
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
          :disabled="!canRedo"
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
            role="radio"
            @click="setColor(color.value)"
          ><span aria-hidden="true"></span></button>
          <button
            type="button"
            class="annotation-color annotation-color-add"
            :title="label('annotationAddColor', 'Add color')"
            :aria-label="label('annotationAddColor', 'Add color')"
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
        'is-creating': activeTool === AnnotationEditorType.HIGHLIGHT
          || activeTool === AnnotationEditorType.FREETEXT
          || activeTool === AnnotationEditorType.INK,
        'is-ink': activeTool === AnnotationEditorType.INK,
      }"
      @contextmenu.capture="preventInkContextMenu"
      @pointerdown.capture="handleInkPointerDown"
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
  background: #1b1f25;
}

.annotation-toolbar {
  z-index: 12;
  display: flex;
  min-height: 42px;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 5px 8px;
  border-bottom: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(24, 28, 34, 0.98);
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
  border-radius: 7px;
  padding: 0 8px;
  background: transparent;
  color: #cbd5e1;
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
  border-color: rgba(96, 165, 250, 0.52);
  background: rgba(59, 130, 246, 0.16);
  color: #fff;
}

.annotation-tool.active,
.annotation-action:not(.quiet) {
  border-color: rgba(96, 165, 250, 0.45);
  background: rgba(59, 130, 246, 0.2);
  color: #dbeafe;
}

.annotation-tool.danger:hover:not(:disabled) {
  border-color: rgba(248, 113, 113, 0.52);
  background: rgba(239, 68, 68, 0.16);
  color: #fecaca;
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
  background: rgba(148, 163, 184, 0.22);
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

.annotation-color span {
  width: 14px;
  height: 14px;
  border: 1px solid rgba(255, 255, 255, 0.55);
  border-radius: 50%;
  background: var(--annotation-color);
  box-shadow: 0 0 0 1px rgba(15, 23, 42, 0.45);
}

.annotation-color:hover,
.annotation-color.active {
  border-color: rgba(255, 255, 255, 0.85);
  background: rgba(148, 163, 184, 0.16);
}

.annotation-color.active span {
  box-shadow: 0 0 0 2px #1e293b, 0 0 0 3px #e2e8f0;
}

.annotation-color-add {
  color: #cbd5e1;
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
  color: #94a3b8;
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.annotation-status.dirty { color: #facc15; }
.annotation-status.error { color: #fca5a5; }

.annotation-pdf-container {
  position: absolute;
  inset: 42px 0 0;
  overflow: auto;
  background: #3b4049;
}

/* Selection is explicit. While a creation tool is active, existing editors are
   transparent to the pointer so a click or stroke always creates at that spot. */
.annotation-pdf-container.is-creating :deep(.annotationEditorLayer > :is(.freeTextEditor, .inkEditor, .highlightEditor, .stampEditor, .signatureEditor)) {
  pointer-events: none !important;
}

.annotation-pdf-container.is-creating :deep(.annotationEditorLayer .editToolbar) {
  pointer-events: none !important;
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
  color: #94a3b8;
  font-size: 13px;
  text-align: center;
}

.annotation-placeholder.error { color: #fca5a5; }

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
