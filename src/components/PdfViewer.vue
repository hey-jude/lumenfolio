<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import * as pdfjsLib from 'pdfjs-dist/legacy/build/pdf.mjs'
import pdfWorkerUrl from 'pdfjs-dist/legacy/build/pdf.worker.mjs?url'
import 'pdfjs-dist/legacy/web/pdf_viewer.css'
import { buildLinkedBlocksByPage, canUseLinkedHover } from '../translationLinking'
import { testAttrs } from '../testAttrs'

installPdfRuntimePolyfills()
pdfjsLib.GlobalWorkerOptions.workerSrc = pdfWorkerUrl
const pdfRuntimeOptions = {
  cMapUrl: '/pdfjs/cmaps/',
  cMapPacked: true,
  standardFontDataUrl: '/pdfjs/standard_fonts/',
}

const RENDER_BUFFER_PX = 900
const MIN_SCALE = 0.7
const MAX_SCALE = 2.4
const HOVER_ASSIST_ENABLED = false
const ASSIST_RAIL_COLLAPSED_WIDTH = 40
const ASSIST_RAIL_EXPANDED_WIDTH = 132
const ASSIST_RAIL_VERTICAL_WIDTH = 58
const ASSIST_RAIL_VERTICAL_HEIGHT = 108
const ASSIST_RAIL_HEIGHT = 40
const ASSIST_RAIL_GAP = 10
const ASSIST_RAIL_MARGIN = 12
const TRANSLATION_POPOVER_WIDTH = 340
const TRANSLATION_POPOVER_MAX_HEIGHT = 320
const SELECTION_TOOLBAR_WIDTH = 120
const SELECTION_TOOLBAR_HEIGHT = 40
const SELECTION_TOOLBAR_GAP = 10
const INDEX_YIELD_EVERY_PAGES = 2
const DEFAULT_PAGE_SIZE = { width: 720, height: 960 }
const PAGE_GAP_PX = 22

const props = defineProps({
  document: {
    type: Object,
    required: true,
  },
  pdfPath: {
    type: String,
    default: '',
  },
  activePage: {
    type: Number,
    default: 1,
  },
  activeHighlight: {
    type: Object,
    default: null,
  },
  activeTranslation: {
    type: Object,
    default: null,
  },
  linkedBlocks: {
    type: Array,
    default: () => [],
  },
  linkedHoverEnabled: {
    type: Boolean,
    default: false,
  },
  externalLinkedBlockHover: {
    type: Object,
    default: null,
  },
  selectionLocked: {
    type: Boolean,
    default: false,
  },
  selectionActionsEnabled: {
    type: Boolean,
    default: true,
  },
  ui: {
    type: Object,
    required: true,
  },
})

const emit = defineEmits([
  'loaded',
  'load-failed',
  'index-started',
  'index-complete',
  'index-failed',
  'page-change',
  'selection',
  'ask-selection',
  'translate-selection',
  'note-selection',
  'close-translation',
  'retry-translation',
  'state-change',
  'linked-block-hover',
])

const pdfScroll = ref(null)
const pageRefs = ref([])
const loading = ref(false)
const error = ref('')
const scale = ref(1.18)
const currentPage = ref(1)
const pageCount = ref(0)
const selectionText = ref('')
const selectionBboxList = ref([])
const selectionPage = ref(0)
const selectionSourceVersion = ref('')
const toolbarPosition = ref({ left: 0, top: 0 })
const copyStatus = ref('idle')
const translationPreviewOpen = ref(false)
const isScrolling = ref(false)
const isSelectingText = ref(false)
const pageViewports = ref([])
const defaultPageSize = ref({ ...DEFAULT_PAGE_SIZE })
const assistBlocksByPage = ref(new Map())
const pageTextItemsByPage = ref(new Map())
const hoveredAssistBlock = ref(null)
const hoveredLinkedBlock = ref(null)
const assistRailHovered = ref(false)
const pdfDocument = shallowRef(null)
const renderedPageKeys = shallowRef(new Set())

let loadRun = 0
let renderRun = 0
let indexRun = 0
let assistHideTimer = null
let assistPointerFrame = 0
let pendingAssistPointer = null
let linkedPointerFrame = 0
let pendingLinkedPointer = null
let selectionStartPoint = null
let scrollbarFadeTimer = null
let copyStatusTimer = null
let pageSyncSuppressTimer = null
let suppressScrollPageChange = false
const renderTasks = new Map()
const textLayerTasks = new Map()

function installPdfRuntimePolyfills() {
  const streamPrototype = globalThis.ReadableStream?.prototype
  if (!streamPrototype || streamPrototype[Symbol.asyncIterator]) return

  Object.defineProperty(streamPrototype, Symbol.asyncIterator, {
    configurable: true,
    writable: true,
    value: async function* readReadableStream() {
      const reader = this.getReader()
      try {
        while (true) {
          const { done, value } = await reader.read()
          if (done) return
          yield value
        }
      } finally {
        reader.releaseLock()
      }
    },
  })
}

const canGoPrevious = computed(() => currentPage.value > 1)
const canGoNext = computed(() => currentPage.value < pageCount.value)
const assistInteractionBlocked = computed(() => (
  !HOVER_ASSIST_ENABLED
  || props.selectionLocked
  || isSelectingText.value
))
const assistRailVisible = computed(() => (
  HOVER_ASSIST_ENABLED
  && Boolean(hoveredAssistBlock.value)
  && !props.activeTranslation
))
const assistRailGeometry = computed(() => {
  const block = hoveredAssistBlock.value
  const pageEl = getPageElement(block?.page)
  if (!block || !pageEl) {
    return {
      iconLeft: 0,
      expandedLeft: 0,
      top: 0,
      horizontal: true,
      expandedWidth: ASSIST_RAIL_COLLAPSED_WIDTH,
      expandedHeight: ASSIST_RAIL_HEIGHT,
    }
  }

  const rect = getBlockBounds(bboxListToRects(block.page, block.bboxList))
  const pageLeft = pageEl.offsetLeft
  const pageTop = pageEl.offsetTop
  const pageBottom = pageTop + pageEl.clientHeight
  const blockRight = pageLeft + rect.x + rect.width
  const visibleBounds = getVisibleBoundsInPages()
  const minIconLeft = visibleBounds.left
  const maxIconLeft = Math.max(minIconLeft, visibleBounds.right - ASSIST_RAIL_COLLAPSED_WIDTH)
  const iconLeft = clamp(blockRight + ASSIST_RAIL_GAP, minIconLeft, maxIconLeft)
  const horizontal = iconLeft + ASSIST_RAIL_EXPANDED_WIDTH <= visibleBounds.right
  const expandedWidth = horizontal ? ASSIST_RAIL_EXPANDED_WIDTH : ASSIST_RAIL_VERTICAL_WIDTH
  const expandedHeight = horizontal ? ASSIST_RAIL_HEIGHT : ASSIST_RAIL_VERTICAL_HEIGHT
  const minExpandedLeft = visibleBounds.left
  const maxExpandedLeft = Math.max(minExpandedLeft, visibleBounds.right - expandedWidth)
  const expandedLeft = clamp(iconLeft, minExpandedLeft, maxExpandedLeft)
  const minTop = Math.max(pageTop + 8, visibleBounds.top)
  const maxTop = Math.max(
    minTop,
    Math.min(pageBottom - expandedHeight - 8, visibleBounds.bottom - expandedHeight),
  )

  return {
    iconLeft,
    expandedLeft,
    horizontal,
    expandedWidth,
    expandedHeight,
    top: clamp(
      pageTop + rect.y + rect.height / 2 - ASSIST_RAIL_HEIGHT / 2,
      minTop,
      maxTop,
    ),
  }
})
const assistRailPosition = computed(() => ({
  left: assistRailHovered.value ? assistRailGeometry.value.expandedLeft : assistRailGeometry.value.iconLeft,
  top: assistRailGeometry.value.top,
}))
const assistRailIsVertical = computed(() => !assistRailGeometry.value.horizontal)
const selectionToolbarVisible = computed(() => (
  props.selectionActionsEnabled
  && Boolean(selectionText.value)
  && !props.activeTranslation
))
const selectionToolbarStyle = computed(() => ({
  left: `${toolbarPosition.value.left}px`,
  top: `${toolbarPosition.value.top}px`,
}))
const translationMeta = computed(() => {
  const translation = props.activeTranslation
  if (!translation) return ''
  const parts = []
  if (translation.provider) parts.push(`${props.ui.translationProvider}: ${translation.provider}`)
  if (translation.cached) parts.push(props.ui.translationCached)
  return parts.join(' · ')
})
const copyButtonLabel = computed(() => (
  copyStatus.value === 'copied' ? props.ui.copied : props.ui.copy
))
const translationPopoverVisible = computed(() => Boolean(props.activeTranslation))
const translationPopoverStyle = computed(() => {
  const translation = props.activeTranslation
  const pageEl = getPageElement(translation?.source?.page)
  const fallback = {
    left: `${Math.max(16, getVisibleBoundsInPages().right - TRANSLATION_POPOVER_WIDTH - 16)}px`,
    top: `${Math.max(16, getVisibleBoundsInPages().top + 16)}px`,
  }
  if (!translation?.source || !pageEl) return fallback

  const bounds = getBlockBounds(
    bboxListToRects(translation.source.page, translation.source.bboxList || []),
  )
  const pageLeft = pageEl.offsetLeft
  const pageTop = pageEl.offsetTop
  const visibleBounds = getVisibleBoundsInPages()
  const preferredLeft = pageLeft + bounds.x + bounds.width + 14
  const pinnedRightLeft = Math.max(
    visibleBounds.left,
    visibleBounds.right - TRANSLATION_POPOVER_WIDTH,
  )
  const left = preferredLeft + TRANSLATION_POPOVER_WIDTH <= visibleBounds.right
    ? preferredLeft
    : pinnedRightLeft
  const top = clamp(
    pageTop + bounds.y,
    visibleBounds.top,
    Math.max(visibleBounds.top, visibleBounds.bottom - TRANSLATION_POPOVER_MAX_HEIGHT),
  )

  return {
    left: `${left}px`,
    top: `${top}px`,
  }
})
const assistRailHitZoneStyle = computed(() => {
  const block = hoveredAssistBlock.value
  const pageEl = getPageElement(block?.page)
  if (!block || !pageEl) return { left: '0px', top: '0px', width: '0px', height: '0px' }

  const blockBounds = getBlockBounds(bboxListToRects(block.page, block.bboxList))
  const pageLeft = pageEl.offsetLeft
  const pageTop = pageEl.offsetTop
  const blockRight = pageLeft + blockBounds.x + blockBounds.width
  const blockTop = pageTop + blockBounds.y
  const blockBottom = pageTop + blockBounds.y + blockBounds.height
  const railLeft = assistRailGeometry.value.expandedLeft
  const railTop = assistRailGeometry.value.top
  const iconLeft = assistRailGeometry.value.iconLeft
  const railWidth = assistRailGeometry.value.expandedWidth
  const railHeight = assistRailGeometry.value.expandedHeight
  const paddingX = 14
  const paddingY = 12

  const left = Math.max(0, Math.min(blockRight, railLeft, iconLeft) - paddingX)
  const right = Math.max(blockRight, railLeft + railWidth, iconLeft + ASSIST_RAIL_COLLAPSED_WIDTH) + paddingX
  const top = Math.max(0, Math.min(blockTop, railTop) - paddingY)
  const bottom = Math.max(blockBottom, railTop + railHeight) + paddingY

  return {
    left: `${left}px`,
    top: `${top}px`,
    width: `${Math.max(0, right - left)}px`,
    height: `${Math.max(railHeight + 8, bottom - top)}px`,
  }
})
const linkedBlocksByPage = computed(() => {
  return buildLinkedBlocksByPage(props.linkedBlocks)
})
const linkedInteractionBlocked = computed(() => (
  !canUseLinkedHover({
    linkedHoverEnabled: props.linkedHoverEnabled,
    selectionLocked: props.selectionLocked,
    isSelectingText: isSelectingText.value,
    hasSelectionText: Boolean(selectionText.value),
    translationPreviewOpen: translationPreviewOpen.value,
    hasActiveTranslation: Boolean(props.activeTranslation),
  })
))

watch([() => props.document.id, () => props.pdfPath], loadPdf, { immediate: true })

watch([currentPage, pageCount, scale, loading, error], emitViewerState)

watch(() => props.activePage, (page) => {
  if (!page || page === currentPage.value || !pdfDocument.value) return
  goToPage(page, {
    behavior: 'auto',
    emitPageChange: false,
    suppressScrollPageChange: true,
  })
})

watch(() => props.activeHighlight, (highlight) => {
  if (!highlight?.page || !pdfDocument.value) return
  goToHighlight(highlight)
})

watch(() => props.activeTranslation?.id, () => {
  resetCopyStatus()
})

watch(() => props.selectionLocked, (locked) => {
  if (locked) clearAssistHover({ immediate: true })
  if (locked) clearLinkedHover()
})

watch(() => props.linkedHoverEnabled, (enabled) => {
  if (!enabled) clearLinkedHover()
})

watch(() => props.linkedBlocks, () => {
  if (!hoveredLinkedBlock.value) return
  const pageBlocks = linkedBlocksByPage.value.get(hoveredLinkedBlock.value.page) || []
  if (!pageBlocks.some((block) => block.blockId === hoveredLinkedBlock.value.blockId)) {
    clearLinkedHover()
  }
})

watch(scale, async () => {
  if (!pdfDocument.value) return
  await resetPageViewportsForScale()
  await nextTick()
  goToPage(currentPage.value, {
    behavior: 'auto',
    emitPageChange: false,
    suppressScrollPageChange: true,
  })
  renderVisiblePages()
})

onMounted(() => {
  window.addEventListener('pointerdown', handleGlobalPointerDown, true)
  document.addEventListener('pointerdown', handleGlobalPointerDown, true)
  window.addEventListener('keydown', handleGlobalKeyDown)
})

onBeforeUnmount(() => {
  loadRun += 1
  renderRun += 1
  indexRun += 1
  window.removeEventListener('pointerdown', handleGlobalPointerDown, true)
  document.removeEventListener('pointerdown', handleGlobalPointerDown, true)
  window.removeEventListener('keydown', handleGlobalKeyDown)
  clearAssistHideTimer()
  clearScrollbarFadeTimer()
  clearCopyStatusTimer()
  if (pageSyncSuppressTimer) window.clearTimeout(pageSyncSuppressTimer)
  cancelAssistPointerFrame()
  cancelLinkedPointerFrame()
  cancelOngoingRender()
  pdfDocument.value?.destroy?.()
})

async function loadPdf() {
  loadRun += 1
  renderRun += 1
  const run = loadRun
  clearSelection()
  resetRenderedState()
  cancelOngoingRender()
  error.value = ''
  pageCount.value = 0
  currentPage.value = 1
  pageRefs.value = []
  pageViewports.value = []
  defaultPageSize.value = { ...DEFAULT_PAGE_SIZE }
  assistBlocksByPage.value = new Map()
  pageTextItemsByPage.value = new Map()
  pdfDocument.value?.destroy?.()
  pdfDocument.value = null

  if (!props.document?.id) return

  loading.value = true
  try {
    const bytes = props.pdfPath
      ? await invoke('read_pdf_artifact_bytes', { path: props.pdfPath })
      : await invoke('read_pdf_bytes', { docId: props.document.id })
    if (run !== loadRun) return
    const data = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)
    const task = pdfjsLib.getDocument({ data, ...pdfRuntimeOptions })
    const loadedPdf = await task.promise
    if (run !== loadRun) {
      loadedPdf.destroy?.()
      return
    }

    pdfDocument.value = loadedPdf
    pageCount.value = loadedPdf.numPages
    await prepareInitialPageViewport(Math.min(Math.max(props.activePage || 1, 1), pageCount.value))
    if (run !== loadRun) return

    emit('loaded', { pageCount: pageCount.value })
    loading.value = false
    await nextTick()
    if (run !== loadRun) return
    goToPage(Math.min(Math.max(props.activePage || 1, 1), pageCount.value), { behavior: 'auto' })
    renderVisiblePages()

    if (shouldUseExistingIndex(pageCount.value)) {
      emit('index-complete', {
        pageCount: pageCount.value,
        indexVersion: props.document.currentIndexVersion,
        treeReady: true,
        visualIndexStatus: props.document.visualIndexStatus || 'pending',
        visualIndexVersion: props.document.visualIndexVersion || 0,
        visualIndexError: props.document.visualIndexError || '',
      })
      return
    }
    if (props.document.backendIndexFailed) {
      queueDocumentIndex(loadedPdf, pageCount.value, run)
    }
  } catch (err) {
    if (run !== loadRun) return
    error.value = err?.message || String(err)
    loading.value = false
    emit('load-failed', { error: error.value })
  }
}

async function prepareInitialPageViewport(preferredPage = 1) {
  const pdf = pdfDocument.value
  if (!pdf) return
  renderRun += 1
  cancelOngoingRender()
  resetRenderedState()
  assistBlocksByPage.value = new Map()
  pageTextItemsByPage.value = new Map()
  pageViewports.value = Array.from({ length: pdf.numPages })
  await ensurePageViewport(preferredPage)
}

async function resetPageViewportsForScale() {
  const pdf = pdfDocument.value
  if (!pdf) return
  renderRun += 1
  cancelOngoingRender()
  resetRenderedState()
  assistBlocksByPage.value = new Map()
  pageTextItemsByPage.value = new Map()
  pageViewports.value = Array.from({ length: pdf.numPages })
  await ensurePageViewport(currentPage.value || 1)
}

async function ensurePageViewport(pageNumber) {
  const pdf = pdfDocument.value
  if (!pdf) return null
  const clampedPage = Math.min(Math.max(Number(pageNumber) || 1, 1), pdf.numPages)
  const existing = pageViewports.value[clampedPage - 1]
  if (existing && existing.scale === scale.value) return existing
  const page = await pdf.getPage(clampedPage)
  const viewport = page.getViewport({ scale: scale.value })
  const nextViewports = [...pageViewports.value]
  nextViewports[clampedPage - 1] = viewport
  pageViewports.value = nextViewports
  if (viewport.width && viewport.height) {
    defaultPageSize.value = {
      width: viewport.width,
      height: viewport.height,
    }
  }
  return viewport
}

function resetRenderedState() {
  renderedPageKeys.value = new Set()
}

async function renderPage(pageNumber) {
  const pdf = pdfDocument.value
  const pageEl = getPageElement(pageNumber)
  if (!pdf || !pageEl) return

  const clampedPage = Math.min(Math.max(pageNumber, 1), pdf.numPages)
  const renderKey = getRenderKey(clampedPage)
  if (renderedPageKeys.value.has(renderKey) || renderTasks.has(clampedPage)) return

  const run = renderRun
  const renderToken = Symbol(`render-${clampedPage}`)
  renderTasks.set(clampedPage, { token: renderToken, task: null })
  try {
    const page = await pdf.getPage(clampedPage)
    if (run !== renderRun) return

    const viewport = page.getViewport({ scale: scale.value })
    pageViewports.value[clampedPage - 1] = viewport
    const canvas = pageEl.querySelector('canvas')
    const textLayerEl = pageEl.querySelector('.pdf-text-layer')
    if (!canvas || !textLayerEl) return

    const context = canvas.getContext('2d')
    const outputScale = window.devicePixelRatio || 1
    canvas.width = Math.floor(viewport.width * outputScale)
    canvas.height = Math.floor(viewport.height * outputScale)
    canvas.style.width = `${viewport.width}px`
    canvas.style.height = `${viewport.height}px`
    textLayerEl.innerHTML = ''
    textLayerEl.style.width = `${viewport.width}px`
    textLayerEl.style.height = `${viewport.height}px`

    const renderTask = page.render({
      canvasContext: context,
      viewport,
      transform: outputScale === 1 ? null : [outputScale, 0, 0, outputScale, 0, 0],
    })
    renderTasks.set(clampedPage, { token: renderToken, task: renderTask })
    await renderTask.promise
    if (renderTasks.get(clampedPage)?.token === renderToken) {
      renderTasks.delete(clampedPage)
    }

    if (run !== renderRun) return
    const textContent = await page.getTextContent()
    const textItems = buildTextItems(textContent, viewport)
    mergePageTextItems(clampedPage, textItems)
    mergeAssistPageIndex(buildPageIndex(clampedPage, viewport, textItems))
    const textLayer = new pdfjsLib.TextLayer({
      textContentSource: textContent,
      container: textLayerEl,
      viewport,
    })
    textLayerTasks.set(clampedPage, textLayer)
    await textLayer.render()
    textLayerTasks.delete(clampedPage)

    const rendered = new Set(renderedPageKeys.value)
    rendered.add(renderKey)
    renderedPageKeys.value = rendered
  } catch (err) {
    if (isCancellationError(err)) return
    if (run === renderRun) {
      error.value = err?.message || String(err)
      emit('load-failed', { error: error.value })
    }
  } finally {
    if (renderTasks.get(clampedPage)?.token === renderToken) {
      renderTasks.delete(clampedPage)
    }
    textLayerTasks.delete(clampedPage)
  }
}

function renderVisiblePages() {
  if (!pdfScroll.value || !pdfDocument.value || loading.value || error.value) return
  const { start, end } = visiblePageRange(RENDER_BUFFER_PX, 2)
  for (let pageNo = start; pageNo <= end; pageNo += 1) {
    renderPage(pageNo)
  }
}

function getRenderKey(pageNumber) {
  return `${pageNumber}@${scale.value}`
}

function goToPage(pageNumber, options = {}) {
  const page = Math.min(Math.max(Number(pageNumber) || 1, 1), pageCount.value || 1)
  const pageEl = getPageElement(page)
  const scroller = pdfScroll.value
  if (!pageEl || !scroller) return
  if (options.suppressScrollPageChange) suppressProgrammaticPageChanges()
  scroller.scrollTo({
    top: Math.max(0, pageEl.offsetTop - 18),
    behavior: options.behavior || 'smooth',
  })
  setCurrentPage(page, { emit: options.emitPageChange !== false })
  renderPage(page)
}

function currentScrollAnchor() {
  const scroller = pdfScroll.value
  if (!scroller || !pageCount.value) return null
  const anchorOffset = scrollAnchorOffset(scroller)
  const anchorTop = scroller.scrollTop + anchorOffset
  const anchorPage = scrollAnchorPage(anchorTop)
  if (!anchorPage) return null
  const progress = (anchorTop - anchorPage.element.offsetTop) / Math.max(1, anchorPage.element.offsetHeight)
  return {
    page: anchorPage.page,
    progress: Math.min(1, Math.max(0, progress)),
  }
}

function scrollToPageProgress(pageNumber, progress = 0, options = {}) {
  const page = Math.min(Math.max(Number(pageNumber) || 1, 1), pageCount.value || 1)
  const pageEl = getPageElement(page)
  const scroller = pdfScroll.value
  if (!pageEl || !scroller) return
  if (options.suppressScrollPageChange) suppressProgrammaticPageChanges()
  const normalizedProgress = Math.min(1, Math.max(0, Number(progress) || 0))
  const anchorOffset = scrollAnchorOffset(scroller)
  const maxTop = Math.max(0, scroller.scrollHeight - scroller.clientHeight)
  const targetTop = Math.min(
    maxTop,
    Math.max(0, pageEl.offsetTop + (pageEl.offsetHeight * normalizedProgress) - anchorOffset),
  )
  if (Math.abs(scroller.scrollTop - targetTop) > 1) {
    scroller.scrollTo({
      top: targetTop,
      behavior: options.behavior || 'auto',
    })
  }
  setCurrentPage(page, { emit: options.emitPageChange !== false })
  renderPage(page)
}

function scrollAnchorOffset(scroller) {
  return Math.min(scroller.clientHeight * 0.42, 360)
}

function scrollAnchorPage(anchorTop) {
  let best = null
  let bestDistance = Number.POSITIVE_INFINITY
  for (let page = 1; page <= pageCount.value; page += 1) {
    const element = getPageElement(page)
    if (!element) continue
    const pageTop = element.offsetTop
    const pageBottom = pageTop + element.offsetHeight
    if (anchorTop >= pageTop && anchorTop <= pageBottom) {
      return { page, element }
    }
    const distance = Math.min(Math.abs(anchorTop - pageTop), Math.abs(anchorTop - pageBottom))
    if (distance < bestDistance) {
      best = { page, element }
      bestDistance = distance
    }
  }
  return best
}

function suppressProgrammaticPageChanges(duration = 420) {
  suppressScrollPageChange = true
  if (pageSyncSuppressTimer) window.clearTimeout(pageSyncSuppressTimer)
  pageSyncSuppressTimer = window.setTimeout(() => {
    suppressScrollPageChange = false
    pageSyncSuppressTimer = null
  }, duration)
}

async function goToHighlight(highlight) {
  const page = Math.min(Math.max(Number(highlight.page) || 1, 1), pageCount.value || 1)
  const pageEl = getPageElement(page)
  const scroller = pdfScroll.value
  if (!pageEl || !scroller) return

  await renderPage(page)
  await nextTick()

  const firstRect = bboxListToRects(page, highlight.bboxList || [])[0]
  const top = firstRect
    ? pageEl.offsetTop + firstRect.y - 96
    : pageEl.offsetTop - 18
  scroller.scrollTo({
    top: Math.max(0, top),
    behavior: 'smooth',
  })
  setCurrentPage(page)
}

function setScale(nextScale) {
  const normalizedScale = Number(nextScale)
  if (!Number.isFinite(normalizedScale)) return scale.value
  const clampedScale = Math.min(MAX_SCALE, Math.max(MIN_SCALE, Number(normalizedScale.toFixed(2))))
  if (clampedScale === scale.value) return scale.value
  scale.value = clampedScale
  return scale.value
}

function zoomBy(delta) {
  return setScale(scale.value + delta)
}

function handleScroll() {
  clearAssistHover({ immediate: true })
  if (!suppressScrollPageChange) updateCurrentPageFromScroll()
  renderVisiblePages()
  emitViewerState()
  showScrollbarWhileScrolling()
}

function showScrollbarWhileScrolling() {
  isScrolling.value = true
  clearScrollbarFadeTimer()
  scrollbarFadeTimer = window.setTimeout(() => {
    isScrolling.value = false
    scrollbarFadeTimer = null
  }, 700)
}

function clearScrollbarFadeTimer() {
  if (!scrollbarFadeTimer) return
  window.clearTimeout(scrollbarFadeTimer)
  scrollbarFadeTimer = null
}

function updateCurrentPageFromScroll() {
  const anchor = currentScrollAnchor()
  if (anchor?.page) setCurrentPage(anchor.page)
}

function visiblePageRange(bufferPx = 0, extraPages = 0) {
  const scroller = pdfScroll.value
  const total = Number(pageCount.value || 0)
  if (!scroller || total <= 0) return { start: 1, end: 0 }
  const top = Math.max(0, scroller.scrollTop - bufferPx)
  const bottom = scroller.scrollTop + scroller.clientHeight + bufferPx
  const first = firstPageWithBottomAfter(top)
  const last = lastPageWithTopBefore(bottom)
  if (first && last) {
    const start = Math.max(1, first - extraPages)
    const end = Math.min(total, Math.max(start, last + extraPages))
    return { start, end }
  }
  return estimatedVisiblePageRange(bufferPx, extraPages)
}

function currentVisiblePages(extraPages = 0) {
  const { start, end } = visiblePageRange(0, extraPages)
  if (!start || end < start) return []
  const pages = []
  for (let page = start; page <= end; page += 1) {
    pages.push(page)
  }
  return pages
}

function firstPageWithBottomAfter(offset) {
  let low = 1
  let high = Number(pageCount.value || 0)
  let answer = 0
  while (low <= high) {
    const mid = Math.floor((low + high) / 2)
    const pageEl = getPageElement(mid)
    if (!pageEl) return 0
    if (pageEl.offsetTop + pageEl.offsetHeight >= offset) {
      answer = mid
      high = mid - 1
    } else {
      low = mid + 1
    }
  }
  return answer
}

function lastPageWithTopBefore(offset) {
  let low = 1
  let high = Number(pageCount.value || 0)
  let answer = 0
  while (low <= high) {
    const mid = Math.floor((low + high) / 2)
    const pageEl = getPageElement(mid)
    if (!pageEl) return 0
    if (pageEl.offsetTop <= offset) {
      answer = mid
      low = mid + 1
    } else {
      high = mid - 1
    }
  }
  return answer
}

function estimatedVisiblePageRange(bufferPx = 0, extraPages = 0) {
  const scroller = pdfScroll.value
  const total = Number(pageCount.value || 0)
  if (!scroller || total <= 0) return { start: 1, end: 0 }
  const span = Math.max(1, (defaultPageSize.value.height || DEFAULT_PAGE_SIZE.height) + PAGE_GAP_PX)
  const start = Math.min(
    total,
    Math.max(1, Math.floor(Math.max(0, scroller.scrollTop - bufferPx) / span) + 1 - extraPages),
  )
  const end = Math.min(
    total,
    Math.ceil((scroller.scrollTop + scroller.clientHeight + bufferPx) / span) + extraPages,
  )
  return { start, end: Math.max(start, end) }
}

function setCurrentPage(page, options = {}) {
  if (!page || page === currentPage.value) return
  currentPage.value = page
  if (options.emit === false) return
  emit('page-change', page)
}

function emitViewerState() {
  emit('state-change', {
    currentPage: currentPage.value,
    pageCount: pageCount.value,
    visiblePages: currentVisiblePages(),
    scale: scale.value,
    canGoPrevious: canGoPrevious.value,
    canGoNext: canGoNext.value,
    loading: loading.value,
    error: Boolean(error.value),
  })
}

function handleSelection(pageNumber) {
  window.setTimeout(() => {
    const selection = window.getSelection()
    const text = selection?.toString().trim() || ''
    const pageEl = findSelectionPageElement(selection) || getPageElement(pageNumber)
    isSelectingText.value = false
    if (!props.selectionActionsEnabled) {
      selectionStartPoint = null
      return
    }
    if (!text || !pageEl) {
      clearSelection()
      return
    }

    const page = getPageNumberForElement(pageEl)
    const pageRect = pageEl.getBoundingClientRect()
    const anchorRect = getSelectionAnchorRect(selection, pageRect)
    if (!anchorRect || !page) {
      clearSelection()
      return
    }

    const citation = normalizeManualSelection(selection, text, page, pageRect, selectionStartPoint)
    if (!citation?.bboxList?.length || !citation.text) {
      clearSelection()
      return
    }
    clearAssistHover({ immediate: true })
    setManualSelection(citation, page, anchorRect, pageEl)
    emit('selection', {
      text: citation.text,
      page,
      bboxList: citation.bboxList,
      sourceType: 'selection',
      sourceVersion: citation.sourceVersion,
    })
  }, 0)
}

function handleSelectionStart(event) {
  if (event.button !== 0) return
  if (props.activeTranslation) emit('close-translation')
  clearSelection()
  selectionStartPoint = getPointerPagePoint(event)
  isSelectingText.value = true
  clearAssistHover({ immediate: true })
}

function handlePdfPointerDownCapture(event) {
  if (event.button !== 0) return
  const target = event.target instanceof Element ? event.target : null
  if (target?.closest('.assist-rail')) return

  clearAssistHover({ immediate: true })
  isSelectingText.value = Boolean(target?.closest('.pdf-page-host'))
}

function getPointerPagePoint(event) {
  const pageEl = event.currentTarget instanceof Element
    ? event.currentTarget.closest('.pdf-page-host')
    : null
  if (!pageEl) return null
  const page = getPageNumberForElement(pageEl)
  const rect = pageEl.getBoundingClientRect()
  return {
    page,
    x: clamp(event.clientX - rect.left, 0, rect.width),
    y: clamp(event.clientY - rect.top, 0, rect.height),
  }
}

function findSelectionPageElement(selection) {
  const anchor = selection?.anchorNode
  let element = anchor instanceof Element ? anchor : anchor?.parentElement
  while (element && !element.classList?.contains('pdf-page-host')) {
    element = element.parentElement
  }
  return element || null
}

function getPageNumberForElement(element) {
  const page = Number(element?.dataset?.page)
  return Number.isFinite(page) ? page : 0
}

function handleGlobalPointerDown(event) {
  const target = event.target instanceof Element ? event.target : null
  if (target?.closest('.selection-toolbar')) return
  if (props.activeTranslation) {
    if (target?.closest('.translation-popover')) return
    emit('close-translation')
    return
  }
  if (!selectionText.value && !translationPreviewOpen.value) return
  clearSelection()
}

function handleGlobalKeyDown(event) {
  if (event.key === 'Escape') {
    clearSelection()
    clearAssistHover({ immediate: true })
    if (props.activeTranslation) emit('close-translation')
  }
}

function setManualSelection(citation, page, anchorRect, pageEl) {
  const visibleBounds = getVisibleBoundsInPages()
  const selectionBounds = getBlockBounds(citation.selectionRects || [])
  const selectionLeft = selectionBounds.width ? selectionBounds.x : anchorRect.x
  const selectionTop = selectionBounds.height ? selectionBounds.y : anchorRect.y
  const selectionBottom = selectionBounds.height
    ? selectionBounds.y + selectionBounds.height
    : anchorRect.y
  const centerLeft = pageEl.offsetLeft + selectionLeft + selectionBounds.width / 2 - SELECTION_TOOLBAR_WIDTH / 2
  const aboveTop = pageEl.offsetTop + selectionTop - SELECTION_TOOLBAR_HEIGHT - SELECTION_TOOLBAR_GAP
  const belowTop = pageEl.offsetTop + selectionBottom + SELECTION_TOOLBAR_GAP
  const hasSpaceAbove = aboveTop >= visibleBounds.top
  const hasSpaceBelow = belowTop + SELECTION_TOOLBAR_HEIGHT <= visibleBounds.bottom
  const preferredTop = hasSpaceAbove || !hasSpaceBelow ? aboveTop : belowTop
  const maxLeft = Math.max(visibleBounds.left, visibleBounds.right - SELECTION_TOOLBAR_WIDTH)
  const maxTop = Math.max(visibleBounds.top, visibleBounds.bottom - SELECTION_TOOLBAR_HEIGHT)

  selectionText.value = citation.text
  selectionPage.value = page
  selectionBboxList.value = citation.bboxList
  selectionSourceVersion.value = citation.sourceVersion || ''
  toolbarPosition.value = {
    left: clamp(centerLeft, visibleBounds.left, maxLeft),
    top: clamp(preferredTop, visibleBounds.top, maxTop),
  }
  window.getSelection()?.removeAllRanges()
}

function emitManualSelectionAction(eventName) {
  if (!selectionText.value || !selectionBboxList.value.length || !selectionPage.value) return
  emit(eventName, {
    text: selectionText.value,
    page: selectionPage.value,
    blockId: '',
    bboxList: selectionBboxList.value,
    sourceType: 'selection',
    sourceVersion: selectionSourceVersion.value || 'manual-selection',
  })
  clearSelection()
}

async function copyActiveTranslation() {
  const text = String(props.activeTranslation?.translatedText || '').trim()
  if (!text) return
  try {
    await navigator.clipboard?.writeText(text)
    copyStatus.value = 'copied'
    clearCopyStatusTimer()
    copyStatusTimer = window.setTimeout(() => {
      copyStatus.value = 'idle'
      copyStatusTimer = null
    }, 1400)
  } catch (err) {
    console.warn('Failed to copy translation', err)
  }
}

function resetCopyStatus() {
  clearCopyStatusTimer()
  copyStatus.value = 'idle'
}

function clearCopyStatusTimer() {
  if (!copyStatusTimer) return
  window.clearTimeout(copyStatusTimer)
  copyStatusTimer = null
}

function getSelectionAnchorRect(selection, pageRect) {
  if (!selection.rangeCount) return null
  const firstRange = selection.getRangeAt(0)
  const rect = Array.from(firstRange.getClientRects()).find((item) => item.width > 2 && item.height > 2)
  if (!rect) return null

  const clippedLeft = Math.max(rect.left, pageRect.left)
  const clippedTop = Math.max(rect.top, pageRect.top)
  if (clippedLeft >= pageRect.right || clippedTop >= pageRect.bottom) return null

  return {
    x: clippedLeft - pageRect.left,
    y: clippedTop - pageRect.top,
  }
}

function normalizeManualSelection(selection, rawText, pageNumber, pageRect, startPoint = null) {
  const selectionRects = getSelectionPageRects(selection, pageRect)
  if (!selectionRects.length) return null
  const visualSelection = extractVisualSelection(pageNumber, selectionRects, startPoint, pageRect)
  const visualText = visualSelection.text
  const text = chooseManualSelectionText(rawText, visualText, selectionRects, visualSelection.columnFiltered)
  const normalizedVisualText = normalizeExtractedText(visualText)
  const selectionSourceRects = normalizedVisualText && text === normalizedVisualText
    ? visualSelection.rects
    : selectionRects
  const bboxList = rectsToBboxList(selectionSourceRects, pageRect)
  if (!bboxList.length) return null

  return {
    text,
    bboxList,
    selectionRects: selectionSourceRects,
    sourceType: 'selection',
    sourceVersion: visualSelection.columnFiltered && normalizedVisualText && text === normalizedVisualText
      ? 'manual-column-v1'
      : normalizedVisualText && text === normalizedVisualText
        ? 'manual-visual-v2'
        : 'manual-dom-v1',
  }
}

function chooseManualSelectionText(rawText, visualText, selectionRects, preferVisual = false) {
  const normalizedRaw = normalizeExtractedText(rawText)
  const normalizedVisual = normalizeExtractedText(visualText)
  if (!normalizedVisual) return normalizedRaw
  if (preferVisual) return normalizedVisual
  if (selectionRects.length <= 1 && normalizedRaw) return normalizedRaw
  if (normalizedRaw && visualExtractionLooksIncomplete(normalizedRaw, normalizedVisual)) {
    return normalizedRaw
  }
  return normalizedVisual
}

function visualExtractionLooksIncomplete(rawText, visualText) {
  const rawLength = compactComparableText(rawText).length
  const visualLength = compactComparableText(visualText).length
  if (rawLength < 24) return false
  return visualLength > 0 && visualLength < rawLength * 0.62
}

function compactComparableText(text) {
  return String(text || '').replace(/\s+/g, '')
}

function extractVisualSelection(pageNumber, selectionRects, startPoint = null, pageRect = null) {
  const textItems = pageTextItemsByPage.value.get(pageNumber) || []
  if (!textItems.length || !selectionRects.length) {
    return { text: '', rects: [], columnFiltered: false }
  }

  const selectedItems = textItems
    .filter((item) => selectionRects.some((rect) => shouldIncludeTextItem(item, rect)))
  const filteredItems = filterItemsToStartColumn(selectedItems, startPoint, pageNumber, pageRect)
  const columnFiltered = filteredItems.length > 0 && filteredItems.length < selectedItems.length
  const selectedRects = filteredItems.map(textItemToPageRect)
  const text = joinVisualTextItems(
    filteredItems
    .sort((left, right) => Math.abs(left.y - right.y) < Math.max(3, Math.min(left.height, right.height) * 0.55)
      ? left.x - right.x
      : left.y - right.y),
  )

  return { text, rects: selectedRects, columnFiltered }
}

function filterItemsToStartColumn(items, startPoint, pageNumber, pageRect) {
  if (!items.length || !startPoint || !pageRect) return items
  if (!startPoint.page || startPoint.page !== pageNumber) return items
  const bounds = getBlockBounds(items)
  if (bounds.width < pageRect.width * 0.55) return items
  if (!selectionLooksLikeMultipleColumns(items, pageRect)) return items

  const clusters = clusterItemsByColumn(items, pageRect)
  if (clusters.length <= 1) return items

  const chosen = clusters
    .map((cluster) => ({
      ...cluster,
      distance: distanceToRange(startPoint.x, cluster.left, cluster.right),
    }))
    .sort((left, right) => left.distance - right.distance)[0]
  return chosen?.items?.length ? chosen.items : items
}

function selectionLooksLikeMultipleColumns(items, pageRect) {
  const lines = buildSelectionLines(items)
  if (lines.length < 2) return false
  const pageCenter = pageRect.width / 2
  let leftLines = 0
  let rightLines = 0
  let crossingLines = 0

  for (const line of lines) {
    const width = line.x2 - line.x1
    if (line.x1 < pageCenter && line.x2 > pageCenter) crossingLines += 1
    if (line.x2 <= pageCenter || (line.x1 < pageCenter && width < pageRect.width * 0.46)) leftLines += 1
    if (line.x1 >= pageCenter || (line.x2 > pageCenter && width < pageRect.width * 0.46)) rightLines += 1
  }

  if (crossingLines > 0) return false
  return leftLines >= 2 && rightLines >= 2
}

function buildSelectionLines(items) {
  const medianHeight = medianNumber(items.map((item) => item.height).filter(Number.isFinite)) || 10
  const sorted = [...items].sort((left, right) => Math.abs(left.y - right.y) < Math.max(3, medianHeight * 0.55)
    ? left.x - right.x
    : left.y - right.y)
  const lines = []
  for (const item of sorted) {
    const last = lines.at(-1)
    const sameLine = last && Math.abs(last.y - item.y) <= Math.max(3, medianHeight * 0.55)
    if (!sameLine) {
      lines.push({
        y: item.y,
        x1: item.x,
        x2: item.x + item.width,
      })
      continue
    }
    last.x1 = Math.min(last.x1, item.x)
    last.x2 = Math.max(last.x2, item.x + item.width)
  }
  return lines
}

function clusterItemsByColumn(items, pageRect) {
  const medianHeight = medianNumber(items.map((item) => item.height).filter(Number.isFinite)) || 10
  const gapThreshold = Math.max(36, medianHeight * 3.2, pageRect.width * 0.06)
  const sorted = [...items].sort((left, right) => (left.x + left.width / 2) - (right.x + right.width / 2))
  const clusters = []

  for (const item of sorted) {
    const last = clusters.at(-1)
    if (!last) {
      clusters.push(createColumnCluster(item))
      continue
    }

    const gap = item.x - last.right
    if (gap > gapThreshold) {
      clusters.push(createColumnCluster(item))
      continue
    }

    last.items.push(item)
    last.left = Math.min(last.left, item.x)
    last.right = Math.max(last.right, item.x + item.width)
  }

  return clusters.filter((cluster) => cluster.items.length > 0)
}

function createColumnCluster(item) {
  return {
    left: item.x,
    right: item.x + item.width,
    items: [item],
  }
}

function distanceToRange(value, left, right) {
  if (value < left) return left - value
  if (value > right) return value - right
  return 0
}

function textItemToPageRect(item) {
  return {
    x: item.x,
    y: item.y,
    width: item.width,
    height: item.height,
  }
}

function shouldIncludeTextItem(item, selectionRect) {
  const overlap = rectIntersectionArea(item, selectionRect)
  if (overlap <= 0) return false
  const itemArea = Math.max(1, item.width * item.height)
  const selectionArea = Math.max(1, selectionRect.width * selectionRect.height)
  const itemOverlapRatio = overlap / itemArea
  const selectionOverlapRatio = overlap / selectionArea
  return itemOverlapRatio >= 0.08 || selectionOverlapRatio >= 0.04
}

function joinVisualTextItems(items) {
  if (!items.length) return ''
  const medianHeight = medianNumber(items.map((item) => item.height).filter(Number.isFinite)) || 10
  const lines = []

  for (const item of items) {
    const last = lines.at(-1)
    const sameLine = last && Math.abs(last.y - item.y) <= Math.max(3, medianHeight * 0.55)
    if (!sameLine) {
      lines.push({
        y: item.y,
        text: item.text,
        x2: item.x + item.width,
      })
      continue
    }

    const gap = item.x - last.x2
    const separator = shouldJoinHyphenated(last.text, item.text) ? '' : gap > medianHeight * 0.18 ? ' ' : ''
    last.text = `${last.text}${separator}${item.text}`.replace(/\s+/g, ' ').trim()
    last.x2 = Math.max(last.x2, item.x + item.width)
  }

  return normalizeExtractedText(lines.map((line) => line.text).join('\n'))
}

function shouldJoinHyphenated(leftText, rightText) {
  return /[A-Za-z]-$/.test(leftText) && /^[a-z]/.test(rightText)
}

function normalizeExtractedText(text) {
  return String(text || '')
    .replace(/\u00a0/g, ' ')
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n[ \t]+/g, '\n')
    .replace(/[ \t]{2,}/g, ' ')
    .trim()
}

function getSelectionPageRects(selection, pageRect) {
  if (!selection?.rangeCount) return []
  const rects = []

  for (let index = 0; index < selection.rangeCount; index += 1) {
    const range = selection.getRangeAt(index)
    rects.push(...Array.from(range.getClientRects()))
  }

  return normalizeSelectionRects(
    rects
    .map((rect) => clientRectToPageRect(rect, pageRect))
      .filter(Boolean),
  )
}

function normalizeSelectionRects(rects) {
  const usefulRects = rects
    .filter((rect) => rect.width > 1 && rect.height > 1)
    .sort((left, right) => Math.abs(left.y - right.y) < Math.max(2, Math.min(left.height, right.height) * 0.5)
      ? left.x - right.x
      : left.y - right.y)

  const lines = []
  for (const rect of usefulRects) {
    const last = lines.at(-1)
    const sameLine = last && Math.abs(last.y - rect.y) <= Math.max(2, Math.min(last.height, rect.height) * 0.65)
    if (!sameLine) {
      lines.push({ ...rect })
      continue
    }

    const nextRight = Math.max(last.x + last.width, rect.x + rect.width)
    const nextBottom = Math.max(last.y + last.height, rect.y + rect.height)
    last.x = Math.min(last.x, rect.x)
    last.y = Math.min(last.y, rect.y)
    last.width = nextRight - last.x
    last.height = nextBottom - last.y
  }

  return lines
}

function rectsToBboxList(rects, pageRect) {
  return rects
    .map((rect) => normalizeBbox(
      rect.x,
      rect.y,
      rect.x + rect.width,
      rect.y + rect.height,
      pageRect.width || 1,
      pageRect.height || 1,
    ))
    .filter(Boolean)
}

function clearSelection() {
  selectionText.value = ''
  selectionPage.value = 0
  selectionBboxList.value = []
  selectionSourceVersion.value = ''
  selectionStartPoint = null
  translationPreviewOpen.value = false
  window.getSelection()?.removeAllRanges()
}

function handleAssistPointerMove(event, pageNumber) {
  handleLinkedPointerMove(event, pageNumber)
  if (!HOVER_ASSIST_ENABLED) return
  if (assistInteractionBlocked.value || selectionText.value || translationPreviewOpen.value || props.activeTranslation) {
    clearAssistHover({ immediate: true })
    return
  }
  pendingAssistPointer = {
    clientX: event.clientX,
    clientY: event.clientY,
    page: pageNumber,
  }
  if (assistPointerFrame) return

  assistPointerFrame = window.requestAnimationFrame(() => {
    assistPointerFrame = 0
    const pointer = pendingAssistPointer
    pendingAssistPointer = null
    processAssistPointerMove(pointer)
  })
}

function processAssistPointerMove(pointer) {
  if (!pointer || assistInteractionBlocked.value || selectionText.value || translationPreviewOpen.value || props.activeTranslation) {
    clearAssistHover({ immediate: true })
    return
  }
  const pageEl = getPageElement(pointer.page)
  if (!pageEl) return
  const pageRect = pageEl.getBoundingClientRect()
  const x = pointer.clientX - pageRect.left
  const y = pointer.clientY - pageRect.top
  const block = findAssistBlockAtPoint(pointer.page, x, y)

  if (!block) {
    clearAssistHover()
    return
  }

  if (hoveredAssistBlock.value?.id === block.id && hoveredAssistBlock.value?.page === pointer.page) return
  clearAssistHideTimer()
  hoveredAssistBlock.value = {
    ...block,
    page: pointer.page,
  }
}

function handleLinkedPointerMove(event, pageNumber) {
  if (linkedInteractionBlocked.value) {
    clearLinkedHover()
    return
  }
  pendingLinkedPointer = {
    clientX: event.clientX,
    clientY: event.clientY,
    page: pageNumber,
  }
  if (linkedPointerFrame) return

  linkedPointerFrame = window.requestAnimationFrame(() => {
    linkedPointerFrame = 0
    const pointer = pendingLinkedPointer
    pendingLinkedPointer = null
    processLinkedPointerMove(pointer)
  })
}

function processLinkedPointerMove(pointer) {
  if (!pointer || linkedInteractionBlocked.value) {
    clearLinkedHover()
    return
  }
  const pageEl = getPageElement(pointer.page)
  if (!pageEl) return
  const pageRect = pageEl.getBoundingClientRect()
  const x = pointer.clientX - pageRect.left
  const y = pointer.clientY - pageRect.top
  const block = findLinkedBlockAtPoint(pointer.page, x, y)

  if (!block) {
    clearLinkedHover()
    return
  }

  if (
    hoveredLinkedBlock.value?.page === block.page
    && hoveredLinkedBlock.value?.blockId === block.blockId
  ) {
    return
  }
  hoveredLinkedBlock.value = block
  emit('linked-block-hover', {
    page: block.page,
    blockId: block.blockId,
  })
}

function handleAssistPointerLeave() {
  clearLinkedHover()
  if (!HOVER_ASSIST_ENABLED) return
  cancelAssistPointerFrame()
  clearAssistHover()
}

function handleAssistRailEnter() {
  clearAssistHideTimer()
  assistRailHovered.value = true
}

function handleAssistRailLeave() {
  assistRailHovered.value = false
  clearAssistHover()
}

function clearAssistHover({ immediate = false } = {}) {
  cancelAssistPointerFrame()
  clearAssistHideTimer()
  if (immediate) {
    hoveredAssistBlock.value = null
    assistRailHovered.value = false
    return
  }
  assistHideTimer = window.setTimeout(() => {
    if (assistRailHovered.value) return
    hoveredAssistBlock.value = null
  }, 120)
}

function clearAssistHideTimer() {
  if (!assistHideTimer) return
  window.clearTimeout(assistHideTimer)
  assistHideTimer = null
}

function cancelAssistPointerFrame() {
  pendingAssistPointer = null
  if (!assistPointerFrame) return
  window.cancelAnimationFrame(assistPointerFrame)
  assistPointerFrame = 0
}

function clearLinkedHover() {
  cancelLinkedPointerFrame()
  if (!hoveredLinkedBlock.value) return
  hoveredLinkedBlock.value = null
  emit('linked-block-hover', null)
}

function cancelLinkedPointerFrame() {
  pendingLinkedPointer = null
  if (!linkedPointerFrame) return
  window.cancelAnimationFrame(linkedPointerFrame)
  linkedPointerFrame = 0
}

function findAssistBlockAtPoint(pageNumber, x, y) {
  const pageBlocks = assistBlocksByPage.value.get(pageNumber) || []
  return pageBlocks.find((block) => isPointInBlock(pageNumber, x, y, block)) || null
}

function findLinkedBlockAtPoint(pageNumber, x, y) {
  const pageBlocks = linkedBlocksByPage.value.get(pageNumber) || []
  return pageBlocks.find((block) => isPointInBlock(pageNumber, x, y, block)) || null
}

function isPointInBlock(pageNumber, x, y, block) {
  return bboxListToRects(pageNumber, block.bboxList).some((rect) => {
    const paddingX = 2
    const paddingY = 3
    return (
      x >= rect.x - paddingX
      && x <= rect.x + rect.width + paddingX
      && y >= rect.y - paddingY
      && y <= rect.y + rect.height + paddingY
    )
  })
}

function getBlockBounds(rects = []) {
  const bounds = rects.reduce((result, rect) => ({
    left: Math.min(result.left, rect.x),
    top: Math.min(result.top, rect.y),
    right: Math.max(result.right, rect.x + rect.width),
    bottom: Math.max(result.bottom, rect.y + rect.height),
  }), {
    left: Number.POSITIVE_INFINITY,
    top: Number.POSITIVE_INFINITY,
    right: Number.NEGATIVE_INFINITY,
    bottom: Number.NEGATIVE_INFINITY,
  })
  if (![bounds.left, bounds.top, bounds.right, bounds.bottom].every(Number.isFinite)) {
    return { x: 0, y: 0, width: 0, height: 0 }
  }
  return {
    x: bounds.left,
    y: bounds.top,
    width: Math.max(0, bounds.right - bounds.left),
    height: Math.max(0, bounds.bottom - bounds.top),
  }
}

function getVisibleBoundsInPages() {
  const scroller = pdfScroll.value
  const pagesEl = scroller?.querySelector('.pdf-pages')
  if (!scroller || !pagesEl) {
    return {
      left: 0,
      top: 0,
      right: Number.POSITIVE_INFINITY,
      bottom: Number.POSITIVE_INFINITY,
    }
  }

  const scrollerRect = scroller.getBoundingClientRect()
  const pagesRect = pagesEl.getBoundingClientRect()
  return {
    left: scrollerRect.left - pagesRect.left + ASSIST_RAIL_MARGIN,
    top: scrollerRect.top - pagesRect.top + ASSIST_RAIL_MARGIN,
    right: scrollerRect.right - pagesRect.left - ASSIST_RAIL_MARGIN,
    bottom: scrollerRect.bottom - pagesRect.top - ASSIST_RAIL_MARGIN,
  }
}

function emitAssistAction(eventName) {
  const block = hoveredAssistBlock.value
  if (!block?.text) return
  const payload = {
    text: block.text,
    page: block.page,
    blockId: block.id,
    bboxList: block.bboxList || [],
    sourceType: 'paragraph',
  }

  emit(eventName, payload)
  clearAssistHover({ immediate: true })
}

function emitAssistQuote() {
  const block = hoveredAssistBlock.value
  if (!block?.text) return

  emit('selection', {
    text: block.text,
    page: block.page,
    blockId: block.id,
    bboxList: block.bboxList || [],
    sourceType: 'paragraph',
  })
  clearAssistHover({ immediate: true })
}

function cancelOngoingRender() {
  for (const entry of renderTasks.values()) {
    entry?.task?.cancel?.()
  }
  for (const task of textLayerTasks.values()) {
    task?.cancel?.()
  }
  renderTasks.clear()
  textLayerTasks.clear()
}

function isCancellationError(err) {
  return err?.name === 'RenderingCancelledException' || err?.name === 'AbortException'
}

function shouldUseExistingIndex(totalPages) {
  return props.document.indexStatus === 'indexed'
    && Number(props.document.pageCount || 0) === totalPages
    && Number(props.document.indexVersion || 0) === Number(props.document.currentIndexVersion || 0)
    && Boolean(props.document.treeReady)
}

async function queueDocumentIndex(pdf, totalPages, run) {
  const activeIndexRun = ++indexRun
  emit('index-started')
  try {
    const outlines = await buildDocumentOutline(pdf)
    const pages = []
    for (let pageNo = 1; pageNo <= totalPages; pageNo += 1) {
      if (run !== loadRun || activeIndexRun !== indexRun) return
      const page = await pdf.getPage(pageNo)
      const viewport = page.getViewport({ scale: 1 })
      const textContent = await page.getTextContent()
      pages.push(buildPageIndex(pageNo, viewport, buildTextItems(textContent, viewport)))
      if (pageNo % INDEX_YIELD_EVERY_PAGES === 0) {
        await yieldToBrowser()
      }
    }

    if (run !== loadRun || activeIndexRun !== indexRun) return
    const result = await invoke('upsert_document_index', {
      input: {
        documentId: props.document.id,
        pageCount: totalPages,
        pages,
        outlines,
      },
    })
    emit('index-complete', {
      pageCount: result?.pageCount || totalPages,
      indexVersion: result?.indexVersion,
      treeReady: result?.treeReady ?? true,
      visualIndexStatus: result?.visualIndexStatus || 'pending',
      visualIndexVersion: result?.visualIndexVersion || 0,
      visualIndexError: result?.visualIndexError || '',
    })
  } catch (err) {
    if (run !== loadRun || activeIndexRun !== indexRun) return
    emit('index-failed', { error: err?.message || String(err) })
  }
}

function yieldToBrowser() {
  return new Promise((resolve) => {
    window.setTimeout(resolve, 0)
  })
}

async function buildDocumentOutline(pdf) {
  if (!pdf?.getOutline) return []
  const rawOutline = await pdf.getOutline().catch(() => null)
  if (!Array.isArray(rawOutline) || rawOutline.length === 0) return []

  const result = []
  let orderIndex = 0

  async function visit(items, level) {
    for (const item of items || []) {
      const title = String(item?.title || '').replace(/\s+/g, ' ').trim()
      const pageStart = await resolveOutlinePage(pdf, item?.dest)
      if (title && pageStart) {
        result.push({
          title,
          level: Math.max(1, level),
          pageStart,
          orderIndex,
        })
        orderIndex += 1
      }
      if (Array.isArray(item?.items) && item.items.length) {
        await visit(item.items, level + 1)
      }
    }
  }

  await visit(rawOutline, 1)
  return result
}

async function resolveOutlinePage(pdf, dest) {
  try {
    const resolvedDest = typeof dest === 'string' ? await pdf.getDestination(dest) : dest
    if (!Array.isArray(resolvedDest) || !resolvedDest[0]) return null
    const pageIndex = await pdf.getPageIndex(resolvedDest[0])
    return Number.isFinite(pageIndex) ? pageIndex + 1 : null
  } catch {
    return null
  }
}

function mergeAssistPageIndex(pageIndex) {
  const next = new Map(assistBlocksByPage.value)
  next.set(pageIndex.pageNo, pageIndex.blocks)
  assistBlocksByPage.value = next
}

function mergePageTextItems(pageNumber, textItems) {
  const next = new Map(pageTextItemsByPage.value)
  next.set(pageNumber, textItems)
  pageTextItemsByPage.value = next
}

function buildTextItems(textContent, viewport) {
  return textContent.items
    .filter((item) => item.str?.trim())
    .map((item) => textItemToRect(item, viewport))
    .filter((item) => item && isUsableTextRect(item, viewport))
    .sort((left, right) => Math.abs(left.y - right.y) < 4 ? left.x - right.x : left.y - right.y)
}

function buildPageIndex(pageNo, viewport, items) {
  const lines = []
  for (const item of items) {
    const last = lines.at(-1)
    const sameBaseline = last && Math.abs(last.y - item.y) <= Math.max(4, item.height * 0.45)
    const inlineGap = sameBaseline ? item.x - last.x2 : 0
    const sameColumnRun = sameBaseline && inlineGap <= Math.max(24, item.height * 1.8)

    if (!last || !sameColumnRun) {
      lines.push({
        text: item.text,
        x1: item.x,
        y1: item.y,
        x2: item.x + item.width,
        y2: item.y + item.height,
        height: item.height,
        rects: [item],
      })
      continue
    }

    last.text = `${last.text} ${item.text}`.replace(/\s+/g, ' ').trim()
    last.x1 = Math.min(last.x1, item.x)
    last.y1 = Math.min(last.y1, item.y)
    last.x2 = Math.max(last.x2, item.x + item.width)
    last.y2 = Math.max(last.y2, item.y + item.height)
    last.height = Math.max(last.height, item.height)
    last.rects.push(item)
  }

  const paragraphs = groupLinesIntoParagraphs(lines, viewport)
  const indexLines = lines
    .map((line, index) => ({
      lineNo: index + 1,
      text: line.text,
      bboxList: line.rects.map((rect) => (
        normalizeBbox(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height, viewport.width, viewport.height)
      )),
    }))
    .filter((line) => line.text.length > 0 && line.bboxList.length > 0)
  const blocks = paragraphs
    .map((paragraph, index) => ({
      id: `${props.document.id}-p${pageNo}-b${index + 1}`,
      blockIndex: index + 1,
      text: paragraph.lines.map((line) => line.text).join('\n'),
      bboxList: paragraph.lines.flatMap((line) => line.rects).map((rect) => (
        normalizeBbox(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height, viewport.width, viewport.height)
      )),
    }))
    .filter((block) => block.text.length > 0 && block.bboxList.length > 0)

  return {
    pageNo,
    width: viewport.width,
    height: viewport.height,
    text: blocks.map((block) => block.text).join('\n'),
    lines: indexLines,
    blocks,
  }
}

function groupLinesIntoParagraphs(lines, viewport) {
  const medianHeight = medianNumber(lines.map((line) => line.height).filter(Number.isFinite)) || 12
  const lineSpacing = estimateLineSpacing(lines, medianHeight, viewport)
  const paragraphs = []

  for (const line of lines) {
    const candidate = findParagraphCandidate(paragraphs, line, medianHeight, lineSpacing, viewport)
    if (candidate) {
      appendLineToParagraph(candidate, line)
      continue
    }

    paragraphs.push({
      text: line.text,
      x1: line.x1,
      y1: line.y1,
      x2: line.x2,
      y2: line.y2,
      height: line.height,
      lines: [line],
    })
  }

  return paragraphs
    .filter((paragraph) => paragraph.text.trim() && paragraph.lines.length > 0)
    .sort((left, right) => Math.abs(left.y1 - right.y1) < medianHeight * 0.6 ? left.x1 - right.x1 : left.y1 - right.y1)
}

function findParagraphCandidate(paragraphs, line, medianHeight, lineSpacing, viewport) {
  let best = null
  let bestGap = Number.POSITIVE_INFINITY

  for (let index = paragraphs.length - 1; index >= 0; index -= 1) {
    const paragraph = paragraphs[index]
    const previousLine = paragraph.lines.at(-1)
    if (!previousLine) continue
    const verticalGap = line.y1 - previousLine.y2
    const topDelta = line.y1 - previousLine.y1
    if (verticalGap < -Math.max(4, medianHeight * 0.35)) continue
    if (topDelta > Math.max(28, lineSpacing.pitch * 2.1)) break
    if (!shouldMergeLineIntoParagraph(line, paragraph, medianHeight, lineSpacing, viewport)) continue
    if (verticalGap < bestGap) {
      best = paragraph
      bestGap = verticalGap
    }
  }

  return best
}

function shouldMergeLineIntoParagraph(line, paragraph, medianHeight, lineSpacing, viewport) {
  const previousLine = paragraph.lines.at(-1)
  if (!previousLine) return false

  const verticalGap = line.y1 - previousLine.y2
  const topDelta = line.y1 - previousLine.y1
  const lineHeight = Math.max(medianHeight, line.height || 0, previousLine.height || 0)
  const maxInlineGap = Math.max(lineSpacing.gap * 2.2, lineHeight * 0.65)
  const maxInlinePitch = Math.max(lineSpacing.pitch * 1.38, lineHeight * 1.62)
  if (verticalGap > maxInlineGap) return false
  if (topDelta > maxInlinePitch) return false

  const heightRatio = Math.max(line.height, previousLine.height) / Math.max(1, Math.min(line.height, previousLine.height))
  if (heightRatio > 1.35) return false

  const overlap = horizontalOverlapRatio(line, previousLine)
  const leftDelta = Math.abs(line.x1 - previousLine.x1)
  const centerDelta = Math.abs(line.x1 + line.x2 - previousLine.x1 - previousLine.x2) / 2
  const sameColumnWidth = Math.min(viewport.width * 0.42, Math.max(line.x2 - line.x1, previousLine.x2 - previousLine.x1) * 1.25)
  const leftAligned = leftDelta <= Math.max(10, lineHeight * 1.3)
  const centerAligned = centerDelta <= sameColumnWidth * 0.35

  return overlap >= 0.35 || (leftAligned && centerAligned)
}

function estimateLineSpacing(lines, medianHeight, viewport) {
  const gaps = []
  const pitches = []
  const ordered = [...lines].sort((left, right) => Math.abs(left.y1 - right.y1) < 4 ? left.x1 - right.x1 : left.y1 - right.y1)

  for (let index = 1; index < ordered.length; index += 1) {
    const line = ordered[index]
    for (let prevIndex = index - 1; prevIndex >= 0; prevIndex -= 1) {
      const previous = ordered[prevIndex]
      const verticalGap = line.y1 - previous.y2
      if (verticalGap < -Math.max(4, medianHeight * 0.35)) continue
      const pitch = line.y1 - previous.y1
      if (pitch > Math.max(32, medianHeight * 2.2)) break
      if (!isLikelySameColumnLine(line, previous, medianHeight, viewport)) continue
      gaps.push(Math.max(0, verticalGap))
      pitches.push(Math.max(0, pitch))
      break
    }
  }

  return {
    gap: medianNumber(gaps) || Math.max(2, medianHeight * 0.18),
    pitch: medianNumber(pitches) || Math.max(8, medianHeight * 1.18),
  }
}

function isLikelySameColumnLine(line, previousLine, lineHeight, viewport) {
  const overlap = horizontalOverlapRatio(line, previousLine)
  const leftDelta = Math.abs(line.x1 - previousLine.x1)
  const centerDelta = Math.abs(line.x1 + line.x2 - previousLine.x1 - previousLine.x2) / 2
  const sameColumnWidth = Math.min(viewport.width * 0.42, Math.max(line.x2 - line.x1, previousLine.x2 - previousLine.x1) * 1.25)
  return overlap >= 0.35 || (
    leftDelta <= Math.max(10, lineHeight * 1.3)
    && centerDelta <= sameColumnWidth * 0.35
  )
}

function appendLineToParagraph(paragraph, line) {
  paragraph.text = `${paragraph.text}\n${line.text}`.trim()
  paragraph.x1 = Math.min(paragraph.x1, line.x1)
  paragraph.y1 = Math.min(paragraph.y1, line.y1)
  paragraph.x2 = Math.max(paragraph.x2, line.x2)
  paragraph.y2 = Math.max(paragraph.y2, line.y2)
  paragraph.height = Math.max(paragraph.height, line.height)
  paragraph.lines.push(line)
}

function horizontalOverlapRatio(left, right) {
  const overlap = Math.min(left.x2, right.x2) - Math.max(left.x1, right.x1)
  if (overlap <= 0) return 0
  const minWidth = Math.min(left.x2 - left.x1, right.x2 - right.x1)
  return minWidth > 0 ? overlap / minWidth : 0
}

function medianNumber(values) {
  if (!values.length) return 0
  const sorted = [...values].sort((left, right) => left - right)
  const middle = Math.floor(sorted.length / 2)
  return sorted.length % 2 === 0
    ? (sorted[middle - 1] + sorted[middle]) / 2
    : sorted[middle]
}

function textItemToRect(item, viewport) {
  const text = item.str?.trim()
  if (!text) return null

  const transform = pdfjsLib.Util.transform(viewport.transform, item.transform)
  const fontHeight = Math.max(1, Math.hypot(transform[2], transform[3]) || Number(item.height) || 1)
  const width = Math.max(1, (Number(item.width) || Math.hypot(transform[0], transform[1]) || 1) * viewport.scale)

  return {
    text,
    x: transform[4],
    y: Math.max(0, transform[5] - fontHeight),
    width,
    height: fontHeight,
  }
}

function isUsableTextRect(item, viewport) {
  if (![item.x, item.y, item.width, item.height].every(Number.isFinite)) return false
  if (item.width < 1.5 || item.height < 1.5) return false
  if (item.x < -4 || item.y < -4) return false
  if (item.x > viewport.width + 4 || item.y > viewport.height + 4) return false
  if (item.width > viewport.width * 0.95) return false
  if (item.height > viewport.height * 0.08) return false
  return true
}

function roundBbox(value) {
  return Math.round(clamp(value, 0, 1) * 10000) / 10000
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value))
}

function normalizeBbox(x1, y1, x2, y2, pageWidth, pageHeight) {
  return [
    roundBbox(x1 / pageWidth),
    roundBbox(y1 / pageHeight),
    roundBbox(x2 / pageWidth),
    roundBbox(y2 / pageHeight),
  ]
}

function bboxListToRects(pageNumber, bboxList = []) {
  const viewport = pageViewports.value[Number(pageNumber) - 1]
  const pageEl = getPageElement(pageNumber)
  const pageWidth = viewport?.width || pageEl?.clientWidth || 1
  const pageHeight = viewport?.height || pageEl?.clientHeight || 1

  return bboxList
    .map((bbox) => bboxToRect(bbox, pageWidth, pageHeight))
    .filter(Boolean)
}

function bboxToRect(bbox, pageWidth, pageHeight) {
  if (!Array.isArray(bbox) || bbox.length < 4) return null
  const [x1, y1, x2, y2] = bbox.map(Number)
  if (![x1, y1, x2, y2].every(Number.isFinite)) return null
  return {
    x: x1 * pageWidth,
    y: y1 * pageHeight,
    width: Math.max(0, (x2 - x1) * pageWidth),
    height: Math.max(0, (y2 - y1) * pageHeight),
  }
}

function rectIntersectionArea(left, right) {
  const x1 = Math.max(left.x, right.x)
  const y1 = Math.max(left.y, right.y)
  const x2 = Math.min(left.x + left.width, right.x + right.width)
  const y2 = Math.min(left.y + left.height, right.y + right.height)
  return Math.max(0, x2 - x1) * Math.max(0, y2 - y1)
}

function clientRectToPageRect(rect, pageRect) {
  const left = clamp(rect.left - pageRect.left, 0, pageRect.width)
  const top = clamp(rect.top - pageRect.top, 0, pageRect.height)
  const right = clamp(rect.right - pageRect.left, 0, pageRect.width)
  const bottom = clamp(rect.bottom - pageRect.top, 0, pageRect.height)

  if (right - left <= 1 || bottom - top <= 1) return null

  return {
    x: left,
    y: top,
    width: right - left,
    height: bottom - top,
  }
}

function getPageElement(pageNumber) {
  return pageRefs.value[Number(pageNumber) - 1] || null
}

function setPageRef(pageNumber, element) {
  if (element) pageRefs.value[Number(pageNumber) - 1] = element
}

function getPageStyle(pageNumber) {
  const viewport = pageViewports.value[Number(pageNumber) - 1]
  return {
    width: `${viewport?.width || defaultPageSize.value.width}px`,
    height: `${viewport?.height || defaultPageSize.value.height}px`,
  }
}

function highlightRectsForPage(pageNumber) {
  if (!props.activeHighlight || props.activeHighlight.page !== pageNumber) return []
  return bboxListToRects(pageNumber, props.activeHighlight.bboxList || [])
}

function selectionRectsForPage(pageNumber) {
  if (!selectionText.value || selectionPage.value !== pageNumber) return []
  return bboxListToRects(pageNumber, selectionBboxList.value || [])
}

function assistRectsForPage(pageNumber) {
  if (!HOVER_ASSIST_ENABLED) return []
  if (!hoveredAssistBlock.value || hoveredAssistBlock.value.page !== pageNumber) return []
  return bboxListToRects(pageNumber, hoveredAssistBlock.value.bboxList || [])
}

function linkedRectsForPage(pageNumber) {
  const hovered = hoveredLinkedBlock.value?.page === pageNumber
    ? hoveredLinkedBlock.value
    : props.externalLinkedBlockHover?.page === pageNumber
      ? props.externalLinkedBlockHover
      : null
  if (!hovered) return []
  const pageBlocks = linkedBlocksByPage.value.get(pageNumber) || []
  const block = pageBlocks.find((item) => item.blockId === hovered.blockId) || hovered
  return bboxListToRects(pageNumber, block.bboxList || [])
}

defineExpose({
  currentScrollAnchor,
  goToPage,
  scrollToPageProgress,
  setScale,
  zoomBy,
  currentVisiblePages,
})
</script>

<template>
  <div class="pdf-viewer">
    <div
      ref="pdfScroll"
      class="pdf-scroll"
      :class="{ 'is-scrolling': isScrolling }"
      @scroll="handleScroll"
      @mousedown.capture="handlePdfPointerDownCapture"
    >
      <div v-if="loading" class="pdf-state">{{ ui.pdfLoading }}</div>
      <div v-else-if="error" class="pdf-state error">
        <div>{{ ui.pdfLoadFailed }}</div>
        <div class="pdf-error">{{ error }}</div>
        <button type="button" @click="loadPdf">{{ ui.retry }}</button>
      </div>

      <div v-else class="pdf-pages">
        <div
          v-for="pageNumber in pageCount"
          :key="pageNumber"
          :ref="(el) => setPageRef(pageNumber, el)"
          class="pdf-page-host"
          :data-page="pageNumber"
          v-bind="testAttrs('pdf-page', { page: pageNumber })"
          :style="getPageStyle(pageNumber)"
          @mousedown="handleSelectionStart"
          @mouseup="handleSelection(pageNumber)"
          @mousemove="handleAssistPointerMove($event, pageNumber)"
          @mouseleave="handleAssistPointerLeave"
        >
          <canvas class="pdf-canvas"></canvas>
          <div class="textLayer pdf-text-layer"></div>
          <div class="highlight-layer" aria-hidden="true">
            <span
              v-for="(rect, index) in highlightRectsForPage(pageNumber)"
              :key="`active-${pageNumber}-${index}`"
              class="reader-highlight active"
              v-bind="testAttrs('reader-highlight-active', { page: pageNumber })"
              :style="{
                left: `${rect.x}px`,
                top: `${rect.y}px`,
                width: `${rect.width}px`,
                height: `${rect.height}px`,
              }"
            ></span>
            <span
              v-for="(rect, index) in selectionRectsForPage(pageNumber)"
              :key="`selection-${pageNumber}-${index}`"
              class="reader-highlight selection"
              :style="{
                left: `${rect.x}px`,
                top: `${rect.y}px`,
                width: `${rect.width}px`,
                height: `${rect.height}px`,
              }"
            ></span>
            <span
              v-for="(rect, index) in assistRectsForPage(pageNumber)"
              :key="`assist-${pageNumber}-${index}`"
              class="reader-highlight assist"
              :style="{
                left: `${rect.x}px`,
                top: `${rect.y}px`,
                width: `${rect.width}px`,
                height: `${rect.height}px`,
              }"
            ></span>
            <span
              v-for="(rect, index) in linkedRectsForPage(pageNumber)"
              :key="`linked-${pageNumber}-${index}`"
              class="reader-highlight linked"
              v-bind="testAttrs('reader-highlight-linked', { page: pageNumber })"
              :style="{
                left: `${rect.x}px`,
                top: `${rect.y}px`,
                width: `${rect.width}px`,
                height: `${rect.height}px`,
              }"
            ></span>
          </div>
        </div>

        <div
          v-if="assistRailVisible && assistRailHovered"
          class="assist-rail-hit-zone"
          :style="assistRailHitZoneStyle"
          @mouseenter="handleAssistRailEnter"
          @mouseleave="handleAssistRailLeave"
        ></div>

        <div
          v-if="assistRailVisible"
          class="assist-rail"
          :class="{ expanded: assistRailHovered, vertical: assistRailIsVertical }"
          :style="{ left: `${assistRailPosition.left}px`, top: `${assistRailPosition.top}px` }"
          @mouseenter="handleAssistRailEnter"
          @mouseleave="handleAssistRailLeave"
          @mousedown.prevent.stop
          @mouseup.stop
          @click.stop
        >
          <button type="button" class="assist-trigger" :title="ui.quoteParagraph" @click="emitAssistQuote">⌁</button>
          <div class="assist-actions">
            <button type="button" :title="ui.translate" @click="emitAssistAction('translate-selection')">A文</button>
            <button type="button" :title="ui.ask" @click="emitAssistAction('ask-selection')">{{ ui.ask }}</button>
          </div>
        </div>

        <div
          v-if="selectionToolbarVisible"
          class="selection-toolbar"
          :style="selectionToolbarStyle"
          @mousedown.prevent.stop
          @mouseup.stop
          @click.stop
        >
          <button type="button" :title="ui.translate" @click="emitManualSelectionAction('translate-selection')">A文</button>
          <button type="button" :title="ui.ask" @click="emitManualSelectionAction('ask-selection')">{{ ui.ask }}</button>
        </div>

        <aside
          v-if="translationPopoverVisible"
          class="translation-popover"
          :class="activeTranslation.status"
          :style="translationPopoverStyle"
          @mousedown.stop
          @mouseup.stop
          @click.stop
        >
          <div class="translation-popover-head">
            <div>
              <div class="translation-popover-title">{{ ui.translationResult }}</div>
              <div class="translation-popover-subtitle">
                {{ ui.page }} {{ activeTranslation.source?.page || activePage }}
                <template v-if="translationMeta"> · {{ translationMeta }}</template>
              </div>
            </div>
            <button type="button" :title="ui.close" :aria-label="ui.close" @click="emit('close-translation')">×</button>
          </div>

          <div v-if="activeTranslation.status === 'failed'" class="translation-popover-error">
            <div>{{ activeTranslation.error }}</div>
            <div class="translation-popover-hint">{{ ui.translationErrorHint }}</div>
          </div>
          <div v-else class="translation-popover-body">
            {{
              activeTranslation.status === 'running'
                ? ui.translationPending
                : activeTranslation.translatedText
            }}
          </div>

          <details v-if="activeTranslation.source?.text" class="translation-source">
            <summary>{{ ui.showSource }}</summary>
            <div>{{ activeTranslation.source.text }}</div>
          </details>

          <div class="translation-popover-actions">
            <button
              v-if="activeTranslation.status !== 'running'"
              type="button"
              @click="emit('retry-translation')"
            >
              {{ activeTranslation.status === 'failed' ? ui.retry : ui.retranslate }}
            </button>
            <button
              type="button"
              :disabled="activeTranslation.status !== 'succeeded'"
              @click="copyActiveTranslation"
            >
              {{ copyButtonLabel }}
            </button>
          </div>
        </aside>
      </div>
    </div>
  </div>
</template>

<style scoped>
.pdf-viewer {
  height: 100%;
  min-height: 0;
  background: var(--bg-app);
}

.pdf-state button {
  min-height: 30px;
  border-radius: 9px;
  border: 1px solid var(--line-soft);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-primary);
  cursor: pointer;
  padding: 0 10px;
}

.pdf-scroll {
  position: relative;
  height: 100%;
  min-height: 0;
  overflow: auto;
  padding: 18px 18px 28px;
  scrollbar-width: thin;
  scrollbar-color: transparent transparent;
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.025), transparent 120px),
    var(--bg-app);
}

.pdf-scroll::-webkit-scrollbar {
  width: 10px;
  height: 10px;
}

.pdf-scroll::-webkit-scrollbar-track {
  background: transparent;
}

.pdf-scroll::-webkit-scrollbar-thumb {
  border: 2px solid transparent;
  border-radius: 999px;
  background: transparent;
  background-clip: padding-box;
  transition: background-color 0.18s ease;
}

.pdf-scroll:hover::-webkit-scrollbar-thumb,
.pdf-scroll.is-scrolling::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.22);
}

.pdf-scroll:hover,
.pdf-scroll.is-scrolling {
  scrollbar-color: rgba(255, 255, 255, 0.22) transparent;
}

.pdf-pages {
  position: relative;
  min-height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 22px;
}

.pdf-state {
  min-height: 280px;
  display: grid;
  place-content: center;
  gap: 12px;
  color: var(--text-secondary);
  text-align: center;
  max-width: 520px;
  margin: 0 auto;
}

.pdf-state.error {
  color: #ffb3b3;
}

.pdf-error {
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.5;
}

.pdf-page-host {
  position: relative;
  flex: 0 0 auto;
  background: #ffffff;
  box-shadow: 0 22px 52px rgba(0, 0, 0, 0.34);
  overflow: hidden;
}

.pdf-canvas,
.pdf-text-layer,
.highlight-layer {
  position: absolute;
  inset: 0;
}

.pdf-canvas {
  position: relative;
  display: block;
}

.highlight-layer {
  pointer-events: none;
  z-index: 2;
  overflow: hidden;
}

.pdf-text-layer {
  z-index: 3;
}

:deep(.pdf-text-layer *)::selection {
  background: rgba(82, 144, 255, 0.14);
  color: transparent;
}

:deep(.pdf-text-layer *)::-moz-selection {
  background: rgba(82, 144, 255, 0.14);
  color: transparent;
}

.reader-highlight {
  position: absolute;
  border-radius: 2px;
}

.reader-highlight.active {
  background: rgba(245, 180, 24, 0.18);
  outline: 1px solid rgba(214, 164, 20, 0.22);
}

.reader-highlight.assist {
  background: rgba(255, 236, 153, 0.16);
}

.reader-highlight.linked {
  background: rgba(96, 165, 250, 0.13);
  outline: 1px solid rgba(96, 165, 250, 0.28);
}

.reader-highlight.selection {
  background: rgba(82, 144, 255, 0.18);
  outline: 1px solid rgba(82, 144, 255, 0.14);
}

.assist-rail {
  position: absolute;
  z-index: 6;
  height: 40px;
  width: 40px;
  overflow: hidden;
  display: flex;
  align-items: center;
  gap: 4px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  border-radius: 999px;
  background: rgba(24, 28, 34, 0.94);
  box-shadow: 0 12px 24px rgba(15, 23, 42, 0.28);
  transition: width 0.14s ease, height 0.14s ease, border-radius 0.14s ease;
}

.assist-rail-hit-zone {
  position: absolute;
  z-index: 5;
  background: transparent;
}

.assist-rail.expanded {
  width: 190px;
  border-radius: 14px;
}

.assist-rail.vertical {
  flex-direction: column;
  align-items: center;
}

.assist-rail.vertical.expanded {
  width: 58px;
  height: 154px;
  border-radius: 14px;
}

.assist-trigger,
.assist-actions button {
  flex: 0 0 auto;
  width: 36px;
  height: 36px;
  border: none;
  border-radius: 999px;
  background: transparent;
  color: #d6dce6;
  cursor: pointer;
  font-size: 12px;
}

.assist-trigger {
  margin-left: 2px;
  color: #9fbfff;
  font-size: 18px;
}

.assist-rail.vertical .assist-trigger {
  margin-left: 0;
  margin-top: 2px;
}

.assist-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.1s ease;
}

.assist-rail.expanded .assist-actions {
  opacity: 1;
  pointer-events: auto;
}

.assist-actions button {
  width: auto;
  min-width: 36px;
  padding: 0 8px;
}

.assist-rail.vertical .assist-actions {
  flex-direction: column;
}

.assist-rail.vertical .assist-actions button {
  width: 50px;
  min-width: 0;
  padding: 0 4px;
}

.assist-actions button:hover,
.assist-trigger:hover {
  background: rgba(106, 169, 255, 0.16);
  color: #ffffff;
}

.selection-toolbar {
  position: absolute;
  z-index: 7;
  width: 120px;
  height: 40px;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 2px 4px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  border-radius: 14px;
  background: rgba(24, 28, 34, 0.96);
  box-shadow: 0 14px 28px rgba(15, 23, 42, 0.3);
  backdrop-filter: blur(14px);
}

.selection-toolbar button {
  flex: 1 1 0;
  min-width: 0;
  height: 34px;
  border: none;
  border-radius: 10px;
  background: transparent;
  color: #d6dce6;
  cursor: pointer;
  font-size: 12px;
  white-space: nowrap;
}

.selection-toolbar button:hover {
  background: rgba(106, 169, 255, 0.16);
  color: #ffffff;
}

.translation-popover {
  position: absolute;
  z-index: 7;
  width: 340px;
  max-width: min(340px, calc(100vw - 48px));
  max-height: 320px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px;
  border: 1px solid rgba(106, 169, 255, 0.2);
  border-radius: 16px;
  background: rgba(24, 28, 34, 0.96);
  box-shadow: 0 20px 42px rgba(15, 23, 42, 0.34);
  backdrop-filter: blur(16px);
}

.translation-popover.failed {
  border-color: rgba(255, 179, 179, 0.24);
}

.translation-popover-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
}

.translation-popover-title {
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 700;
}

.translation-popover-subtitle {
  margin-top: 4px;
  color: var(--text-muted);
  font-size: 11px;
  line-height: 1.45;
}

.translation-popover-head button,
.translation-popover-actions button {
  min-height: 30px;
  border-radius: 9px;
  border: 1px solid var(--line-soft);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-primary);
  cursor: pointer;
  padding: 0 10px;
}

.translation-popover-head button {
  min-width: 30px;
  padding: 0;
  border-radius: 999px;
  flex: 0 0 auto;
}

.translation-popover-body,
.translation-popover-error {
  flex: 1 1 auto;
  min-height: 0;
  overflow: auto;
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 1.65;
  white-space: pre-wrap;
}

.translation-source {
  flex: 0 0 auto;
  border-top: 1px solid rgba(255, 255, 255, 0.07);
  padding-top: 8px;
  color: var(--text-muted);
  font-size: 12px;
}

.translation-source summary {
  cursor: pointer;
  color: var(--text-secondary);
}

.translation-source div {
  max-height: 110px;
  overflow: auto;
  margin-top: 8px;
  white-space: pre-wrap;
  line-height: 1.55;
}

.translation-popover-error {
  color: #ffb3b3;
}

.translation-popover-hint {
  margin-top: 8px;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.45;
}

.translation-popover-actions {
  flex: 0 0 auto;
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.translation-popover-actions button:disabled {
  opacity: 0.48;
  cursor: not-allowed;
}

</style>
