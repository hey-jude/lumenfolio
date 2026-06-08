<script setup>
import {
  computed,
  defineAsyncComponent,
  h,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from 'vue'
import {
  buildLinkedTranslationBlocks,
  createTranslatedBlockSelection,
} from '../translationLinking'
import lumenfolioLogo from '../assets/lumenfolio-logo-transparent.png'
import { testAttrs } from '../testAttrs'
import { startWindowDrag } from '../windowDrag'

const TRANSLATION_PAGE_REQUEST_COOLDOWN_MS = 5000
const PANE_SCROLL_LINK_STORAGE_KEY = 'lumenfolio:pane-scroll-linked'
const PdfViewerLoading = {
  render: () => h('div', {
    class: 'pdf-viewer-loading',
    'aria-hidden': 'true',
  }),
}
const PdfViewer = defineAsyncComponent({
  loader: () => import('./PdfViewer.vue'),
  loadingComponent: PdfViewerLoading,
  delay: 80,
  timeout: 30000,
})

const props = defineProps({
  document: {
    type: Object,
    required: true,
  },
  translationLanguages: {
    type: Array,
    required: true,
  },
  translationLang: {
    type: String,
    required: true,
  },
  viewMode: {
    type: String,
    required: true,
  },
  activePage: {
    type: Number,
    required: true,
  },
  activeBlockId: {
    type: String,
    default: '',
  },
  activeHighlight: {
    type: Object,
    default: null,
  },
  noteHighlights: {
    type: Array,
    default: () => [],
  },
  hoveredLinkedBlock: {
    type: Object,
    default: null,
  },
  activeTranslation: {
    type: Object,
    default: null,
  },
  pageTranslation: {
    type: Object,
    default: null,
  },
  selectionLocked: {
    type: Boolean,
    default: false,
  },
  inlineTranslateOpen: {
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
})

const emit = defineEmits([
  'update:translationLang',
  'translation-action',
  'cancel-translation',
  'set-view-mode',
  'toggle-inline-translate',
  'select-page',
  'linked-block-hover',
  'request-translation-page',
  'request-pdf-translation-pages',
  'document-loaded',
  'document-load-failed',
  'document-index-started',
  'document-index-complete',
  'document-index-failed',
  'selection',
  'ask-selection',
  'translate-selection',
  'note-selection',
  'close-translation',
  'retry-translation',
  'realign',
])

const showSource = ref(false)
const pdfViewerRef = ref(null)
const translationPdfViewerRef = ref(null)
const translationScrollRef = ref(null)
const translationScrolling = ref(false)
const translationListReady = ref(false)
const paneScrollLinkPreference = ref(loadPaneScrollLinkPreference())
const paneScrollLinked = ref(false)
const pdfViewerState = ref({
  currentPage: 1,
  pageCount: 0,
  visiblePages: [],
  scale: 1.18,
  canGoPrevious: false,
  canGoNext: false,
  loading: true,
  error: false,
})
const translationPdfViewerState = ref({
  currentPage: 1,
  pageCount: 0,
  visiblePages: [],
  scale: 1.18,
  canGoPrevious: false,
  canGoNext: false,
  loading: true,
  error: false,
})
const translationPageRefs = new Map()
const requestedTranslationPageKeys = new Map()
let translationPageObserver = null
let translationVisibleLoadFrame = 0
let translationScrollbarTimer = null
let pdfTranslationPageRequestTimer = null
let lastPdfTranslationPageRequestKey = ''
let translationScrollSyncFrame = 0
let linkedScrollSyncFrame = 0
let translationDrivenPage = 0
let translationDrivenPageTimer = null
let translationListReadyFrame = 0

function loadPaneScrollLinkPreference() {
  try {
    return window.localStorage?.getItem(PANE_SCROLL_LINK_STORAGE_KEY) === 'true'
  } catch {
    return false
  }
}

function savePaneScrollLinkPreference(value) {
  try {
    window.localStorage?.setItem(PANE_SCROLL_LINK_STORAGE_KEY, value ? 'true' : 'false')
  } catch {
    // Ignore storage failures; the in-session state still works.
  }
}

const translationStatusLabel = computed(() => {
  const status = props.document.translation.status
  if (status === 'idle') return props.ui.statusIdle
  if (status === 'queued') return props.ui.statusPending
  if (status === 'pending') return props.ui.statusPending
  if (status === 'running') return props.ui.statusRunning
  if (status === 'succeeded') return props.ui.statusSucceeded
  if (status === 'failed') return props.ui.statusFailed
  return status
})

const translationActionLabel = computed(() => {
  const status = props.document.translation.status
  if (status === 'idle') return props.ui.startTranslation
  if (status === 'pending' || status === 'queued' || status === 'running' || status === 'partial') return props.ui.translating
  if (status === 'failed') return props.ui.retry
  return props.ui.startTranslation
})

const canCancelTranslation = computed(() => {
  const translation = props.document.translation || {}
  if (!['pending', 'queued', 'running', 'partial'].includes(translation.status)) return false
  return Boolean(translation.jobId || translation.pdfJobId)
})
const canOpenTranslationView = computed(() => {
  const translation = props.document.translation || {}
  // 'unsupported' (scanned PDF, no text layer) must keep the pane open so the
  // "Translation not available" notice is visible — otherwise the view just
  // collapses on failure and the user has no idea what happened.
  if (['pending', 'queued', 'running', 'partial', 'succeeded', 'failed', 'unsupported'].includes(translation.status)) {
    return true
  }
  return translation.status === 'canceled' && Boolean(translation.monoPdfPath || translation.dualPdfPath)
})
const canOperateTranslation = computed(() => props.document.chatReady && (isLocalPdf.value || (props.document.pages?.length || 0) > 0))
// Contextual tooltip for the single translate/view toggle (the ◧ button):
// before translating it reads "Translate"; once translated it toggles
// Original/Dual; while running it shows the status.
const translationToggleLabel = computed(() => {
  if (props.document.translation.status === 'succeeded') {
    return props.viewMode === 'dual' ? props.ui.original : props.ui.dual
  }
  if (canCancelTranslation.value) return translationStatusLabel.value
  return translationActionLabel.value
})
const showIndexStatus = computed(() => props.document.indexStatus && props.document.indexStatus !== 'indexed')
const isLocalPdf = computed(() => props.document.source === 'local')
const pageTranslationStatus = computed(() => props.pageTranslation?.status || props.document.translation.status || 'idle')
const translationStreamOpen = computed(() => ['dual', 'translated'].includes(props.viewMode) && canOpenTranslationView.value)
const translationPageCount = computed(() => Math.max(
  Number(pdfViewerState.value.pageCount || 0),
  Number(props.document.pageCount || 0),
  Object.keys(props.document.translation.pages || {}).length,
  1,
))
const translatedPageList = computed(() => {
  if (!translationStreamOpen.value) {
    const blocks = props.pageTranslation?.blocks || []
    return [{
      pageNo: props.activePage,
      translation: props.pageTranslation || null,
      blocks,
    }]
  }
  return Array.from({ length: translationPageCount.value }, (_, index) => {
    const pageNo = index + 1
    const translation = props.document.translation.pages?.[pageNo] || null
    return {
      pageNo,
      translation,
      blocks: translation?.blocks || [],
    }
  })
})

function parseArtifactPages(value) {
  return String(value || '')
    .split(',')
    .flatMap((part) => {
      const token = part.trim()
      if (!token) return []
      const [startValue, endValue] = token.split('-').map((item) => Number(item.trim()))
      if (!Number.isFinite(startValue) || startValue <= 0) return []
      if (!Number.isFinite(endValue) || endValue <= startValue) return [Math.floor(startValue)]
      const pages = []
      for (let page = Math.floor(startValue); page <= Math.floor(endValue); page += 1) {
        pages.push(page)
      }
      return pages
    })
}

const activePartialArtifact = computed(() => {
  const artifacts = props.document.translation?.partialArtifacts || {}
  return artifacts[String(props.activePage)] || artifacts[Number(props.activePage)] || null
})
const translationArtifactScope = computed(() => {
  if (props.document.translation?.pdfArtifactScope === 'full') return 'full'
  return activePartialArtifact.value ? 'partial' : ''
})
const translationArtifactPages = computed(() => {
  if (translationArtifactScope.value === 'partial') {
    return parseArtifactPages(activePartialArtifact.value?.artifactPages || props.activePage)
  }
  return []
})
const partialArtifactPageIndex = computed(() => {
  if (translationArtifactScope.value !== 'partial') return 0
  const active = Number(props.activePage || 0)
  const index = translationArtifactPages.value.indexOf(active)
  return index >= 0 ? index + 1 : 0
})
const translationArtifactPath = computed(() => {
  if (!canOpenTranslationView.value) return ''
  const translation = props.document.translation || {}
  if (translation.pdfArtifactScope === 'full') return translation.monoPdfPath || ''
  if (translationArtifactScope.value === 'partial' && partialArtifactPageIndex.value) {
    return activePartialArtifact.value?.monoPdfPath || ''
  }
  return ''
})
const translationArtifactActivePage = computed(() => {
  if (translationArtifactScope.value === 'partial') return partialArtifactPageIndex.value || 1
  return props.activePage
})
const translationArtifactIsPartial = computed(() => {
  return translationArtifactScope.value === 'partial' && Boolean(translationArtifactPath.value)
})
const translationArtifactOriginalPages = computed(() => {
  if (!translationArtifactIsPartial.value) return []
  return translationArtifactPages.value
})
const translationArtifactDocumentId = computed(() => {
  const pages = translationArtifactOriginalPages.value.join(',')
  return [
    props.document.id,
    'pdf-translation',
    translationArtifactScope.value || 'none',
    pages || 'full',
    translationArtifactPath.value,
  ].join(':')
})
const translationArtifactPageLabel = computed(() => {
  if (!translationArtifactIsPartial.value) return ''
  const pages = translationArtifactOriginalPages.value.join(', ')
  return pages ? `p${pages}` : ''
})
function handleTranslationArtifactPageChange(page) {
  if (!paneScrollLinked.value) return
  if (!translationArtifactIsPartial.value) {
    emit('select-page', page)
    return
  }
  const originalPage = translationArtifactOriginalPages.value[Number(page) - 1]
  if (originalPage) emit('select-page', originalPage)
}

function goToTranslationArtifactPage(page, options = {}) {
  const targetPage = translationArtifactIsPartial.value
    ? translationArtifactActivePage.value
    : page
  translationPdfViewerRef.value?.goToPage(targetPage, options)
}

function syncTranslationArtifactToActivePage() {
  if (!translationArtifactPath.value) return
  translationPdfViewerRef.value?.setScale?.(pdfViewerState.value.scale || 1.18)
  goToTranslationArtifactPage(props.activePage, {
    behavior: 'auto',
    emitPageChange: false,
    suppressScrollPageChange: true,
  })
}

const translationArtifactLabel = computed(() => {
  const translation = props.document.translation || {}
  if (translation.cached) return props.ui.translationCached
  if (translationArtifactIsPartial.value) {
    return translationArtifactPageLabel.value
      ? `${props.ui.translationPartial} ${translationArtifactPageLabel.value}`
      : props.ui.translationPartial
  }
  return translation.phase || pageTranslationStatus.value
})
const translationArtifactDocument = computed(() => ({
  ...props.document,
  id: translationArtifactDocumentId.value,
  pageCount: translationArtifactIsPartial.value
    ? Math.max(translationArtifactOriginalPages.value.length, 1)
    : props.document.pageCount,
  indexStatus: 'indexed',
  indexVersion: props.document.currentIndexVersion || props.document.indexVersion || 0,
  treeReady: true,
  backendIndexFailed: false,
}))
const canLinkPaneScroll = computed(() => (
  props.viewMode === 'dual'
  && canOpenTranslationView.value
  && !translationFullState.value
  && !translationArtifactIsPartial.value
))
const paneScrollLinkLabel = computed(() => {
  if (paneScrollLinked.value) {
    return props.locale === 'zh' ? '解除双栏滚动锁定' : 'Unlock pane scrolling'
  }
  return props.locale === 'zh' ? '锁定双栏滚动' : 'Lock pane scrolling'
})
const hasLoadedTranslationPages = computed(() => (
  translatedPageList.value.some((page) => isTranslationPageLoaded(page.translation))
))
const translationFullState = computed(() => {
  const translation = props.document.translation || {}
  if (translationArtifactPath.value || hasLoadedTranslationPages.value) return null
  if (translation.status === 'unsupported') {
    return {
      tone: 'info',
      title: props.ui.scannedPdfTranslationTitle,
      detail: translation.error || props.ui.scannedPdfTranslationUnsupported,
    }
  }
  if (translation.status === 'failed') {
    return {
      tone: 'error',
      title: props.ui.translationFailed,
      detail: translation.error || props.ui.translationFailed,
    }
  }
  if (['pending', 'queued', 'running', 'partial'].includes(translation.status)) {
    return {
      tone: 'loading',
      title: props.ui.translating,
      detail: translationPhaseLabel.value,
    }
  }
  return null
})
// Keep showing the loading panel for one extra frame after translation data
// first arrives, so the full per-page list mounts off the data-arrival frame
// instead of janking it. See translationListReady watcher below.
const translationPaneState = computed(() => {
  if (translationFullState.value) return translationFullState.value
  if (translationStreamOpen.value && !translationArtifactPath.value && !translationListReady.value) {
    return {
      tone: 'loading',
      title: props.ui.translating,
      detail: translationPhaseLabel.value,
    }
  }
  return null
})
const linkedTranslationBlocks = computed(() => {
  return translatedPageList.value.flatMap((page) => buildLinkedTranslationBlocks({
    viewMode: props.viewMode,
    canOpenTranslationView: canOpenTranslationView.value,
    pageNo: page.pageNo,
    blocks: page.blocks || [],
  }))
})
const translationProgressPercent = computed(() => {
  const total = Number(props.document.translation.total || 0)
  if (!total) return 0
  const done = Number(props.document.translation.progress || 0)
    + Number(props.document.translation.failedBlocks || 0)
  return Math.min(100, Math.max(0, Math.round((done / total) * 100)))
})
const documentProgressSummary = computed(() => {
  const total = Number(props.document.translation.total || 0)
  const done = Number(props.document.translation.progress || 0)
    + Number(props.document.translation.failedBlocks || 0)
  return `${props.ui.documentTranslationProgress} ${done}/${total || '...'}`
})
const translationPhaseLabel = computed(() => {
  const phase = props.document.translation.phase || props.document.translation.status
  if (phase === 'queued' || phase === 'pending') return props.ui.translationPhaseQueued
  if (phase === 'finished' || phase === 'succeeded') return props.ui.translationPhaseFinished
  if (phase === 'failed') return props.ui.statusFailed
  return props.ui.translationPhaseRunning
})
const indexStatusLabel = computed(() => {
  if (props.document.indexStatus === 'indexed') return props.ui.statusIndexed
  if (props.document.indexStatus === 'stale') return props.ui.statusStale
  const percent = Number(props.document.indexProgress?.percent || 0)
  if (percent > 0 && percent < 100) return `${props.ui.statusIndexing} ${percent}%`
  return props.ui.statusIndexing
})
const indexProgressPercent = computed(() => {
  const percent = Number(props.document.indexProgress?.percent || 0)
  return Number.isFinite(percent) ? Math.max(0, Math.min(100, percent)) : 0
})
const activePageData = computed(() => (
  props.document.pages.find((page) => page.page === props.activePage)
  || props.document.pages[0]
  || null
))

function localizedLabel(value) {
  if (!value || typeof value !== 'object') return value
  return value[props.locale] || value.en || Object.values(value)[0]
}

function updatePdfViewerState(state, source = 'source') {
  if (source !== 'source') {
    translationPdfViewerState.value = {
      ...translationPdfViewerState.value,
      ...state,
      visiblePages: Array.isArray(state.visiblePages)
        ? state.visiblePages.map((page) => Number(page)).filter((page) => page > 0)
        : translationPdfViewerState.value.visiblePages,
    }
    scheduleLinkedPaneScrollSync(source)
    return
  }
  pdfViewerState.value = {
    ...pdfViewerState.value,
    ...state,
    visiblePages: Array.isArray(state.visiblePages)
      ? state.visiblePages.map((page) => Number(page)).filter((page) => page > 0)
      : pdfViewerState.value.visiblePages,
  }
  if (source === 'source') requestVisiblePdfTranslationPages()
  scheduleLinkedPaneScrollSync(source)
}

function translationViewportContext() {
  const currentPage = Number(pdfViewerState.value.currentPage || props.activePage || 1)
  const measuredPages = pdfViewerRef.value?.currentVisiblePages?.() || []
  const visiblePages = measuredPages.length
    ? measuredPages
    : (pdfViewerState.value.visiblePages?.length ? pdfViewerState.value.visiblePages : [currentPage])
  return {
    currentPage,
    visiblePages: visiblePages.map((page) => Number(page)).filter((page) => page > 0),
    pageCount: Number(pdfViewerState.value.pageCount || props.document.pageCount || 0),
  }
}

function requestVisiblePdfTranslationPages() {
  const translation = props.document.translation || {}
  if (!['running', 'partial'].includes(translation.status)) return
  if (!translation.pdfJobId || translation.pdfArtifactScope === 'full') return
  const pages = translationViewportContext().visiblePages
  if (!pages.length) return
  const key = `${props.document.id}:${translation.pdfJobId}:${pages.join(',')}`
  if (key === lastPdfTranslationPageRequestKey) return
  lastPdfTranslationPageRequestKey = key
  if (pdfTranslationPageRequestTimer) window.clearTimeout(pdfTranslationPageRequestTimer)
  pdfTranslationPageRequestTimer = window.setTimeout(() => {
    pdfTranslationPageRequestTimer = null
    emit('request-pdf-translation-pages', {
      jobId: translation.pdfJobId,
      pages,
    })
  }, 150)
}

function setTranslationPageRef(pageNo, element) {
  const normalizedPage = Number(pageNo)
  const previous = translationPageRefs.get(normalizedPage)
  if (previous && previous !== element && translationPageObserver) {
    translationPageObserver.unobserve(previous)
  }
  if (element) {
    translationPageRefs.set(normalizedPage, element)
    translationPageObserver?.observe(element)
  } else if (previous) {
    translationPageObserver?.unobserve(previous)
    translationPageRefs.delete(normalizedPage)
  }
}

function requestTranslationPage(pageNo) {
  const normalizedPage = Number(pageNo)
  if (!translationStreamOpen.value || !normalizedPage) return
  const existing = props.document.translation.pages?.[normalizedPage]
  if (isTranslationPageLoaded(existing)) return
  const requestKey = [
    props.document.id,
    props.document.translation.jobId || props.document.translation.providerKey || 'no-job',
    props.translationLang,
    normalizedPage,
  ].join(':')
  const now = Date.now()
  const previousRequestAt = requestedTranslationPageKeys.get(requestKey) || 0
  if (now - previousRequestAt < TRANSLATION_PAGE_REQUEST_COOLDOWN_MS) return
  requestedTranslationPageKeys.set(requestKey, now)
  emit('request-translation-page', normalizedPage)
}

function refreshTranslationObserver() {
  if (translationPageObserver) {
    translationPageObserver.disconnect()
    translationPageObserver = null
  }
  if (!translationStreamOpen.value || !translationScrollRef.value) return
  if (!window.IntersectionObserver) {
    requestVisibleTranslationPages()
    return
  }
  translationPageObserver = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue
      requestTranslationPage(entry.target.dataset.page)
    }
  }, {
    root: translationScrollRef.value,
    rootMargin: '720px 0px',
    threshold: 0.01,
  })
  for (const element of translationPageRefs.values()) {
    translationPageObserver.observe(element)
  }
  requestVisibleTranslationPages()
}

function scheduleTranslationObserverRefresh() {
  nextTick(refreshTranslationObserver)
}

function scheduleVisibleTranslationPageLoad() {
  if (translationPageObserver) return
  if (translationVisibleLoadFrame) return
  translationVisibleLoadFrame = window.requestAnimationFrame(() => {
    translationVisibleLoadFrame = 0
    requestVisibleTranslationPages()
  })
}

function handleTranslationScroll() {
  translationScrolling.value = true
  if (translationScrollbarTimer) window.clearTimeout(translationScrollbarTimer)
  translationScrollbarTimer = window.setTimeout(() => {
    translationScrolling.value = false
    translationScrollbarTimer = null
  }, 760)
  scheduleVisibleTranslationPageLoad()
  scheduleTranslationActivePageSync()
  scheduleLinkedPaneScrollSync('translation-text')
}

function requestVisibleTranslationPages() {
  const scroller = translationScrollRef.value
  if (!translationStreamOpen.value || !scroller) return
  const scrollerRect = scroller.getBoundingClientRect()
  const preloadPx = 720
  const top = scrollerRect.top - preloadPx
  const bottom = scrollerRect.bottom + preloadPx
  for (const [pageNo, element] of translationPageRefs.entries()) {
    const rect = element.getBoundingClientRect()
    if (rect.bottom < top || rect.top > bottom) continue
    requestTranslationPage(pageNo)
  }
}

function scheduleTranslationActivePageSync() {
  if (!paneScrollLinked.value || translationArtifactPath.value) return
  if (translationScrollSyncFrame) return
  translationScrollSyncFrame = window.requestAnimationFrame(() => {
    translationScrollSyncFrame = 0
    const pageNo = currentTranslationAnchorPage()
    if (!pageNo || pageNo === Number(props.activePage)) return
    markTranslationDrivenPage(pageNo)
    emit('select-page', pageNo)
  })
}

function scheduleLinkedPaneScrollSync(source) {
  if (!paneScrollLinked.value || !canLinkPaneScroll.value) return
  if (linkedScrollSyncFrame) window.cancelAnimationFrame(linkedScrollSyncFrame)
  linkedScrollSyncFrame = window.requestAnimationFrame(() => {
    linkedScrollSyncFrame = 0
    if (source === 'source') {
      syncTranslationScrollFromSource()
    } else if (source === 'artifact') {
      syncSourceScrollFromArtifact()
    } else if (source === 'translation-text') {
      syncSourceScrollFromTranslationText()
    }
  })
}

function syncTranslationScrollFromSource() {
  const anchor = pdfViewerRef.value?.currentScrollAnchor?.()
  if (!anchor?.page) return
  if (translationArtifactPath.value) {
    const page = translationArtifactIsPartial.value ? translationArtifactActivePage.value : anchor.page
    translationPdfViewerRef.value?.scrollToPageProgress?.(page, anchor.progress, {
      behavior: 'auto',
      emitPageChange: false,
      suppressScrollPageChange: true,
    })
    return
  }
  scrollTranslationToPageProgress(anchor.page, anchor.progress)
}

function syncSourceScrollFromArtifact() {
  if (translationArtifactIsPartial.value) return
  const anchor = translationPdfViewerRef.value?.currentScrollAnchor?.()
  if (!anchor?.page) return
  markTranslationDrivenPage(anchor.page)
  emit('select-page', anchor.page)
  pdfViewerRef.value?.scrollToPageProgress?.(anchor.page, anchor.progress, {
    behavior: 'auto',
    emitPageChange: false,
    suppressScrollPageChange: true,
  })
}

function syncSourceScrollFromTranslationText() {
  const anchor = currentTranslationAnchor()
  if (!anchor?.page) return
  markTranslationDrivenPage(anchor.page)
  emit('select-page', anchor.page)
  pdfViewerRef.value?.scrollToPageProgress?.(anchor.page, anchor.progress, {
    behavior: 'auto',
    emitPageChange: false,
    suppressScrollPageChange: true,
  })
}

function markTranslationDrivenPage(pageNo, duration = 420) {
  translationDrivenPage = Number(pageNo) || 0
  if (translationDrivenPageTimer) window.clearTimeout(translationDrivenPageTimer)
  translationDrivenPageTimer = window.setTimeout(() => {
    translationDrivenPage = 0
    translationDrivenPageTimer = null
  }, duration)
}

function currentTranslationAnchorPage() {
  return currentTranslationAnchor()?.page || 0
}

function currentTranslationAnchor() {
  const scroller = translationScrollRef.value
  if (!scroller) return null
  const anchorTop = scroller.scrollTop + translationAnchorOffset(scroller)
  let bestPage = 0
  let bestDistance = Number.POSITIVE_INFINITY
  let bestElement = null
  for (const [pageNo, element] of translationPageRefs.entries()) {
    const pageTop = element.offsetTop
    const pageBottom = pageTop + element.offsetHeight
    if (anchorTop >= pageTop && anchorTop <= pageBottom) {
      bestPage = Number(pageNo)
      bestElement = element
      break
    }
    const distance = Math.min(Math.abs(anchorTop - pageTop), Math.abs(anchorTop - pageBottom))
    if (distance < bestDistance) {
      bestDistance = distance
      bestPage = Number(pageNo)
      bestElement = element
    }
  }
  if (!bestPage || !bestElement) return null
  const progress = (anchorTop - bestElement.offsetTop) / Math.max(1, bestElement.offsetHeight)
  return {
    page: bestPage,
    progress: Math.min(1, Math.max(0, progress)),
  }
}

function translationAnchorOffset(scroller) {
  return Math.min(scroller.clientHeight * 0.42, 360)
}

function scrollTranslationToPage(pageNo, behavior = 'auto') {
  const normalizedPage = Number(pageNo)
  const scroller = translationScrollRef.value
  const pageEl = translationPageRefs.get(normalizedPage)
  if (!translationStreamOpen.value || translationArtifactPath.value || !scroller || !pageEl) return
  const targetTop = pageEl.getBoundingClientRect().top
    - scroller.getBoundingClientRect().top
    + scroller.scrollTop
  scroller.scrollTo({
    top: targetTop,
    behavior,
  })
}

function scrollTranslationToPageProgress(pageNo, progress = 0) {
  const normalizedPage = Number(pageNo)
  const scroller = translationScrollRef.value
  const pageEl = translationPageRefs.get(normalizedPage)
  if (!translationStreamOpen.value || translationArtifactPath.value || !scroller || !pageEl) return
  const targetTop = pageEl.getBoundingClientRect().top
    - scroller.getBoundingClientRect().top
    + scroller.scrollTop
  const normalizedProgress = Math.min(1, Math.max(0, Number(progress) || 0))
  const anchorOffset = translationAnchorOffset(scroller)
  const maxTop = Math.max(0, scroller.scrollHeight - scroller.clientHeight)
  const scrollTop = Math.min(
    maxTop,
    Math.max(0, targetTop + (pageEl.offsetHeight * normalizedProgress) - anchorOffset),
  )
  if (Math.abs(scroller.scrollTop - scrollTop) > 1) {
    scroller.scrollTo({
      top: scrollTop,
      behavior: 'auto',
    })
  }
}

function goToPreviousPdfPage() {
  if (!pdfViewerState.value.canGoPrevious) return
  goToSyncedPdfPage(pdfViewerState.value.currentPage - 1)
}

function goToNextPdfPage() {
  if (!pdfViewerState.value.canGoNext) return
  goToSyncedPdfPage(pdfViewerState.value.currentPage + 1)
}

function goToSyncedPdfPage(page) {
  const pageCount = Number(pdfViewerState.value.pageCount || props.document.pageCount || 1)
  const targetPage = Math.min(Math.max(Number(page) || 1, 1), pageCount || 1)
  emit('select-page', targetPage)
  pdfViewerRef.value?.goToPage(targetPage, { emitPageChange: false })
  if (translationArtifactPath.value) {
    goToTranslationArtifactPage(targetPage, {
      behavior: 'auto',
      emitPageChange: false,
      suppressScrollPageChange: true,
    })
  }
}

function togglePaneScrollLink() {
  if (!canLinkPaneScroll.value) return
  const nextLinked = !paneScrollLinked.value
  paneScrollLinkPreference.value = nextLinked
  savePaneScrollLinkPreference(nextLinked)
  paneScrollLinked.value = nextLinked
  if (paneScrollLinked.value) {
    nextTick(() => {
      scrollTranslationToPage(props.activePage)
      if (translationArtifactPath.value) {
        syncTranslationArtifactToActivePage()
      }
    })
  }
}

function zoomPdf(delta) {
  const currentScale = Number(pdfViewerState.value.scale || 1.18)
  const targetScale = Math.min(2.4, Math.max(0.7, Number((currentScale + delta).toFixed(2))))
  pdfViewerRef.value?.setScale?.(targetScale)
  if (translationArtifactPath.value) {
    translationPdfViewerRef.value?.setScale?.(targetScale)
  }
}

function translationPagePlaceholderLabel(translation) {
  if (translation?.status === 'failed') return translation.error || props.ui.translationFailed
  if (props.document.translation.status === 'succeeded') return props.ui.translationPageLoading
  return props.ui.translationPending
}

function selectTranslatedBlock(pageNo, block) {
  const selection = createTranslatedBlockSelection(pageNo, block)
  emit('select-page', selection.page, selection.blockId, selection.source)
}

function isTranslationPageLoaded(translation) {
  return ['succeeded', 'partial', 'failed'].includes(translation?.status)
}

onMounted(() => {
  // eslint-disable-next-line no-console
  console.log(`[pdf-perf] ReaderPane MOUNTED doc=${props.document?.id}`)
  scheduleTranslationObserverRefresh()
})

onBeforeUnmount(() => {
  if (translationPageObserver) translationPageObserver.disconnect()
  if (translationVisibleLoadFrame) window.cancelAnimationFrame(translationVisibleLoadFrame)
  if (translationScrollSyncFrame) window.cancelAnimationFrame(translationScrollSyncFrame)
  if (linkedScrollSyncFrame) window.cancelAnimationFrame(linkedScrollSyncFrame)
  if (translationScrollbarTimer) window.clearTimeout(translationScrollbarTimer)
  if (pdfTranslationPageRequestTimer) window.clearTimeout(pdfTranslationPageRequestTimer)
  if (translationDrivenPageTimer) window.clearTimeout(translationDrivenPageTimer)
  if (translationListReadyFrame) window.cancelAnimationFrame(translationListReadyFrame)
  translationListReadyFrame = 0
  translationPageObserver = null
  translationVisibleLoadFrame = 0
  translationScrollSyncFrame = 0
  linkedScrollSyncFrame = 0
  translationScrollbarTimer = null
  pdfTranslationPageRequestTimer = null
  translationDrivenPageTimer = null
  translationDrivenPage = 0
})

watch([
  translationStreamOpen,
  () => props.document.id,
  () => props.document.translation.jobId,
  () => props.document.translation.providerKey,
  () => props.translationLang,
], () => {
  requestedTranslationPageKeys.clear()
  lastPdfTranslationPageRequestKey = ''
  scheduleTranslationObserverRefresh()
})

// Defer mounting the full per-page translation list by one frame: when the
// list is about to show (stream open, no full-state, no artifact), hold the
// loading panel and flip readiness in a rAF so the heavy mount lands off the
// triggering frame. Reset when the list should no longer be shown.
watch([translationFullState, translationStreamOpen, translationArtifactPath], () => {
  const shouldShowList = translationStreamOpen.value
    && !translationFullState.value
    && !translationArtifactPath.value
  if (!shouldShowList) {
    if (translationListReadyFrame) {
      window.cancelAnimationFrame(translationListReadyFrame)
      translationListReadyFrame = 0
    }
    translationListReady.value = false
    return
  }
  if (translationListReady.value || translationListReadyFrame) return
  translationListReadyFrame = window.requestAnimationFrame(() => {
    translationListReadyFrame = 0
    translationListReady.value = true
  })
}, { immediate: true })

watch([
  translationPageCount,
  () => props.document.id,
], scheduleTranslationObserverRefresh)

watch([
  () => props.activePage,
  translationStreamOpen,
  translationArtifactPath,
], () => {
  if (!paneScrollLinked.value) return
  if (canLinkPaneScroll.value) return
  if (Number(props.activePage) === translationDrivenPage) return
  nextTick(() => scrollTranslationToPage(props.activePage))
})

watch(canLinkPaneScroll, (enabled) => {
  paneScrollLinked.value = enabled && paneScrollLinkPreference.value
  if (paneScrollLinked.value) {
    nextTick(() => {
      scrollTranslationToPage(props.activePage)
      if (translationArtifactPath.value) {
        syncTranslationArtifactToActivePage()
      }
    })
  }
}, { immediate: true })

watch(paneScrollLinkPreference, (preferred) => {
  paneScrollLinked.value = canLinkPaneScroll.value && preferred
})

watch(translationArtifactPath, async (path) => {
  if (!path) return
  await nextTick()
  syncTranslationArtifactToActivePage()
})

watch(translationArtifactActivePage, async () => {
  if (!translationArtifactPath.value) return
  await nextTick()
  syncTranslationArtifactToActivePage()
})
</script>

<template>
  <section class="reader-pane">
    <div class="reader-chrome">
      <!-- Row 1: thin title bar = window drag region + the active document name. -->
      <div class="reader-titlebar" data-tauri-drag-region @mousedown="startWindowDrag">
        <span class="reader-titlebar-name" :title="document.title || document.shortTitle">
          {{ document.shortTitle || document.title || 'PDF' }}
        </span>
      </div>

      <!-- Row 2: translation/page/zoom controls (right). Document switching is
           driven entirely by the left sidebar — there is no separate tab strip. -->
      <div class="reader-tabrow">
        <div class="reader-tab-controls">
          <select
            class="toolbar-select"
            :value="translationLang"
            :title="localizedLabel(translationLanguages.find((item) => item.value === translationLang)?.label)"
            @change="emit('update:translationLang', $event.target.value)"
          >
            <option v-for="item in translationLanguages" :key="item.value" :value="item.value">
              {{ localizedLabel(item.label) }}
            </option>
          </select>

          <button
            v-if="canCancelTranslation"
            class="toolbar-btn stop-action"
            :title="ui.cancel"
            :aria-label="ui.cancel"
            v-bind="testAttrs('cancel-translation')"
            @click="emit('cancel-translation')"
          >
            {{ ui.cancel }}
          </button>

          <button
            class="toolbar-btn split-toggle"
            :class="{ active: viewMode === 'dual', running: canCancelTranslation }"
            :disabled="!canOperateTranslation"
            :title="translationToggleLabel"
            :aria-label="translationToggleLabel"
            v-bind="testAttrs('view-mode-dual')"
            @click="emit('translation-action', translationViewportContext())"
          >
            <span class="toolbar-icon">◧</span>
            <span v-if="canCancelTranslation" class="compact-progress">{{ translationProgressPercent }}%</span>
          </button>

          <template v-if="isLocalPdf">
            <span v-if="showIndexStatus" class="status-chip" :class="document.indexStatus">
              {{ indexStatusLabel }}
            </span>

            <div class="pdf-toolbar-controls">
              <div class="toolbar-control-group page-control-group">
                <button
                  type="button"
                  :title="ui.previousPage"
                  :aria-label="ui.previousPage"
                  :disabled="!pdfViewerState.canGoPrevious"
                  @click="goToPreviousPdfPage"
                >
                  ‹
                </button>
                <span class="page-meter">
                  {{ pdfViewerState.currentPage }} / {{ pdfViewerState.pageCount || document.pageCount || '...' }}
                </span>
                <button
                  type="button"
                  :title="ui.nextPage"
                  :aria-label="ui.nextPage"
                  :disabled="!pdfViewerState.canGoNext"
                  @click="goToNextPdfPage"
                >
                  ›
                </button>
              </div>
              <div class="toolbar-control-group zoom-control-group">
                <button type="button" title="Zoom out" aria-label="Zoom out" @click="zoomPdf(-0.12)">−</button>
                <span class="zoom-meter">{{ Math.round(pdfViewerState.scale * 100) }}%</span>
                <button type="button" title="Zoom in" aria-label="Zoom in" @click="zoomPdf(0.12)">+</button>
              </div>
            </div>
          </template>

          <span v-else class="status-chip" :class="document.translation.status">
            {{ translationStatusLabel }}
            <template v-if="document.translation.total">
              · {{ document.translation.progress }}/{{ document.translation.total }}
            </template>
          </span>
        </div>
      </div>
    </div>

    <div
      v-if="document.translation.status === 'failed' && document.translation.error"
      class="translation-error"
    >
      [ Error ] {{ ui.translationFailed }}: {{ document.translation.error }}
    </div>

    <div class="viewer-stage">
      <template v-if="!document.chatReady && !isLocalPdf">
        <div class="empty-stage">
          <div class="empty-title">{{ ui.preparingDocument }}</div>
          <div class="empty-copy">{{ ui.progressDone }}: {{ ui.progressSummary }}</div>
          <div class="progress-bar">
            <span :style="{ width: `${indexProgressPercent}%` }"></span>
          </div>
          <div class="empty-copy">{{ ui.progressDoing }}: {{ ui.progressDetails }}</div>
        </div>
      </template>

      <template v-else-if="isLocalPdf">
        <div
          class="local-pdf-layout"
          :class="{
            'is-dual': viewMode === 'dual' && canOpenTranslationView,
            'is-translation-only': viewMode === 'translated' && canOpenTranslationView,
          }"
        >
          <div
            v-show="!(viewMode === 'translated' && canOpenTranslationView)"
            class="viewer-card pdf-compare-pane local-pdf-source"
          >
            <PdfViewer
              ref="pdfViewerRef"
              :key="`source-pdf:${document.id}`"
              class="real-pdf-viewer"
              :class="{ 'compare-pdf-viewer': viewMode === 'dual' && canOpenTranslationView }"
              :document="document"
              :active-page="activePage"
              :active-highlight="activeHighlight"
              :note-highlights="noteHighlights"
              :active-translation="activeTranslation"
              :linked-blocks="linkedTranslationBlocks"
              :linked-hover-enabled="false"
              :external-linked-block-hover="null"
              :selection-locked="selectionLocked"
              :ui="ui"
              @loaded="emit('document-loaded', $event)"
              @load-failed="emit('document-load-failed', $event)"
              @index-started="emit('document-index-started', $event)"
              @index-complete="emit('document-index-complete', $event)"
              @index-failed="emit('document-index-failed', $event)"
              @page-change="emit('select-page', $event)"
              @selection="emit('selection', $event)"
              @ask-selection="emit('ask-selection', $event)"
              @translate-selection="emit('translate-selection', $event)"
              @note-selection="emit('note-selection', $event)"
              @close-translation="emit('close-translation')"
              @retry-translation="emit('retry-translation')"
              @state-change="updatePdfViewerState($event, 'source')"
              @linked-block-hover="emit('linked-block-hover', $event)"
            />
          </div>

          <div
            v-if="viewMode === 'dual' && canOpenTranslationView"
            class="pane-link-rail"
            aria-hidden="false"
          >
            <button
              type="button"
              class="pane-link-button"
              :class="{ linked: paneScrollLinked }"
              :disabled="!canLinkPaneScroll"
              :title="paneScrollLinkLabel"
              :aria-label="paneScrollLinkLabel"
              :aria-pressed="paneScrollLinked"
              v-bind="testAttrs('pane-scroll-link-toggle', { linked: paneScrollLinked ? 'true' : 'false' })"
              @click="togglePaneScrollLink"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
              >
                <path d="M9 17H7A5 5 0 0 1 7 7h2" />
                <path d="M15 7h2a5 5 0 0 1 0 10h-2" />
                <line v-if="paneScrollLinked" x1="8" y1="12" x2="16" y2="12" />
              </svg>
            </button>
          </div>

          <div
            v-if="['dual', 'translated'].includes(viewMode) && canOpenTranslationView"
            class="viewer-card translated-reader"
            :class="{ 'translation-wide-pane': viewMode === 'translated' }"
          >
            <PdfViewer
              v-if="translationArtifactPath"
              ref="translationPdfViewerRef"
              class="real-pdf-viewer translated-artifact-viewer"
              v-bind="testAttrs('translated-artifact-viewer')"
              :document="translationArtifactDocument"
              :pdf-path="translationArtifactPath"
              :active-page="translationArtifactActivePage"
              :selection-locked="true"
              :selection-actions-enabled="false"
              :ui="ui"
              @page-change="handleTranslationArtifactPageChange"
              @state-change="updatePdfViewerState($event, 'artifact')"
            />
            <div
              v-else
              ref="translationScrollRef"
              class="translation-page"
              :class="{
                wide: viewMode === 'translated',
                'is-scrolling': translationScrolling,
                'has-full-state': translationPaneState,
              }"
              @scroll="handleTranslationScroll"
            >
              <div
                v-if="translationPaneState"
                class="translation-full-state"
                :class="translationPaneState.tone"
                v-bind="testAttrs('translation-full-state', { status: document.translation.status })"
              >
                <div class="translation-full-logo">
                  <img :src="lumenfolioLogo" alt="" />
                </div>
                <div class="translation-full-title">{{ translationPaneState.title }}</div>
                <div class="translation-full-detail">{{ translationPaneState.detail }}</div>
                <div v-if="translationPaneState.tone === 'loading'" class="translation-full-track">
                  <span :style="{ width: `${Math.max(4, translationProgressPercent)}%` }"></span>
                </div>
                <button
                  v-if="translationPaneState.tone === 'error'"
                  type="button"
                  class="translation-full-retry"
                  @click="emit('translation-action', translationViewportContext())"
                >
                  {{ ui.retry }}
                </button>
              </div>
              <template v-else>
                <section
                  v-for="page in translatedPageList"
                  :key="page.pageNo"
                  :ref="(el) => setTranslationPageRef(page.pageNo, el)"
                  class="translation-page-sheet"
                  v-bind="testAttrs('translation-page', { page: page.pageNo, status: page.translation?.status || 'pending' })"
                  :class="{
                    active: page.pageNo === activePage,
                    pending: !isTranslationPageLoaded(page.translation),
                    failed: page.translation?.status === 'failed',
                  }"
                >
                  <div v-if="isTranslationPageLoaded(page.translation)" class="translation-page-content">
                    <div class="translation-page-title">
                      {{ ui.translatedPdf }} · p{{ page.pageNo }}
                    </div>
                    <button
                      v-for="block in page.blocks || []"
                      :key="block.blockId"
                      type="button"
                      class="translation-page-block"
                      :class="{ active: block.blockId === activeBlockId }"
                      v-bind="testAttrs('translation-block', { page: page.pageNo, blockId: block.blockId })"
                      @click="selectTranslatedBlock(page.pageNo, block)"
                    >
                      <div class="translation-page-block-source">{{ block.sourceText }}</div>
                      <div class="translation-page-block-translation">{{ block.translatedText }}</div>
                    </button>
                  </div>
                  <div v-else class="translation-placeholder" v-bind="testAttrs('translation-placeholder', {
                    page: page.pageNo,
                    status: page.translation?.status || 'pending',
                  })">
                    <span>{{ translationPagePlaceholderLabel(page.translation) }}</span>
                  </div>
                </section>
              </template>
            </div>
          </div>
        </div>
      </template>

      <template v-else-if="viewMode === 'dual' && canOpenTranslationView">
        <div class="single-view">
          <div class="compare-grid">
            <div class="viewer-card">
              <div class="viewer-card-header">
                <span>{{ ui.originalPdf }}</span>
                <span>{{ ui.scrollSynced }}</span>
              </div>
              <div class="page-card">
                <div class="page-heading">{{ activePageData?.heading }}</div>
                <button
                  v-for="block in activePageData?.blocks || []"
                  :key="block.id"
                  class="text-block"
                  :class="{ active: block.id === activeBlockId }"
                  @click="emit('select-page', activePage, block.id)"
                >
                  {{ block.text }}
                </button>
              </div>
            </div>

            <div class="viewer-card">
              <div class="viewer-card-header">
                <span>{{ ui.translatedPdf }}</span>
                <span>{{ ui.scrollSynced }}</span>
              </div>
              <div class="page-card translated">
                <div class="page-heading">{{ activePageData?.heading }}</div>
                <div
                  v-for="block in activePageData?.blocks || []"
                  :key="`${block.id}-translated`"
                  class="text-block"
                  :class="{ active: block.id === activeBlockId }"
                >
                  {{ block.translatedText }}
                </div>
              </div>
            </div>
          </div>
        </div>
      </template>

      <template v-else>
        <div class="single-view">
          <PdfViewer
            v-if="isLocalPdf"
            ref="pdfViewerRef"
            class="real-pdf-viewer"
            :document="document"
            :active-page="activePage"
            :active-highlight="activeHighlight"
            :note-highlights="noteHighlights"
            :active-translation="activeTranslation"
            :selection-locked="selectionLocked"
            :ui="ui"
            @loaded="emit('document-loaded', $event)"
            @load-failed="emit('document-load-failed', $event)"
            @index-started="emit('document-index-started', $event)"
            @index-complete="emit('document-index-complete', $event)"
            @index-failed="emit('document-index-failed', $event)"
            @page-change="emit('select-page', $event)"
            @selection="emit('selection', $event)"
            @ask-selection="emit('ask-selection', $event)"
            @translate-selection="emit('translate-selection', $event)"
            @note-selection="emit('note-selection', $event)"
            @close-translation="emit('close-translation')"
            @retry-translation="emit('retry-translation')"
            @state-change="updatePdfViewerState($event, 'source')"
          />

          <div v-else class="viewer-card">
            <div class="viewer-card-header">
              <span>{{ ui.viewerTitle }}</span>
              <span>100% · {{ ui.fitWidth }}</span>
            </div>
            <div class="page-card">
              <div class="page-heading">{{ activePageData?.heading }}</div>
              <button
                v-for="block in activePageData?.blocks || []"
                :key="block.id"
                class="text-block"
                :class="{ active: block.id === activeBlockId }"
                @click="emit('select-page', activePage, block.id)"
              >
                {{ block.text }}
              </button>

              <div class="selection-toolbar">
                <button class="active-action" @click="emit('toggle-inline-translate')">{{ ui.translate }}</button>
                <button @click="emit('ask-selection')">{{ ui.ask }}</button>
              </div>

              <div v-if="inlineTranslateOpen" class="inline-translation">
                <div class="inline-translation-head">
                  <span>{{ ui.translateTo }}: {{ localizedLabel(translationLanguages.find((item) => item.value === translationLang)?.label) }}</span>
                  <button @click="emit('toggle-inline-translate')">×</button>
                </div>
                <div class="inline-translation-body">
                  {{ activePageData?.blocks?.find((block) => block.id === activeBlockId)?.translatedText }}
                </div>
                <button class="toggle-source" @click="showSource = !showSource">
                  {{ showSource ? ui.hideSource : ui.showSource }}
                </button>
                <div v-if="showSource" class="inline-source">
                  {{ activePageData?.blocks?.find((block) => block.id === activeBlockId)?.text }}
                </div>
              </div>
            </div>
          </div>

          <div v-if="document.pages?.length" class="page-nav">
            <button
              v-for="page in document.pages"
              :key="page.page"
              class="page-chip"
              :class="{ active: page.page === activePage }"
              @click="emit('select-page', page.page)"
            >
              p{{ page.page }}
            </button>
          </div>
        </div>
      </template>
    </div>
  </section>
</template>

<style scoped>
.reader-pane {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-app);
}

.reader-chrome {
  display: grid;
  grid-template-rows: 30px 42px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.075);
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.035), rgba(255, 255, 255, 0.012)),
    var(--bg-app);
}

/* Row 1: thin centered title bar (window drag handle + active doc name). */
.reader-titlebar {
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 0;
  padding: 0 14px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.045);
}

.reader-titlebar-name {
  max-width: 70%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-muted);
  font-size: 12px;
  font-weight: 600;
}

/* Row 2: translation/page/zoom controls, right-aligned. Not a drag region so the
   controls stay clickable. Document switching lives in the left sidebar. */
.reader-tabrow {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 12px;
}

.reader-tab-controls {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-left: auto;
  flex-shrink: 0;
}

.toolbar-select,
.toolbar-btn,
.toolbar-control-group {
  height: 32px;
  border-radius: 8px;
  /* Ghost toolbar: no borders/boxes by default — controls read as icons/labels
     and only show a subtle highlight on hover (or when active). */
  border: none;
  background: transparent;
  color: var(--text-primary);
  white-space: nowrap;
  flex-shrink: 0;
}

.toolbar-select {
  /* Text-only, no dropdown caret; width hugs the selected language name. */
  width: auto;
  max-width: 150px;
  appearance: none;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  outline: none;
  padding: 0 10px;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0;
  text-align: left;
  text-overflow: ellipsis;
}

.toolbar-select:hover,
.toolbar-btn:hover,
.pdf-toolbar-controls button:hover {
  background-color: rgba(255, 255, 255, 0.08);
}

/* The ghost toolbar controls use a calm background highlight instead of a hard
   focus ring — a native <select> keeps focus after a click, so a blue ring would
   linger and look harsh. */
.toolbar-select:focus-visible,
.toolbar-btn:focus-visible,
.pdf-toolbar-controls button:focus-visible {
  outline: none;
  background-color: rgba(255, 255, 255, 0.08);
}

.toolbar-select:hover,
.toolbar-select:focus-visible {
  background-color: rgba(255, 255, 255, 0.08);
}

.toolbar-btn {
  min-width: 32px;
  padding: 0 12px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 7px;
  font-size: 12px;
  font-weight: 700;
}

.toolbar-btn:disabled,
.toolbar-select:disabled,
.pdf-toolbar-controls button:disabled {
  opacity: 0.46;
  cursor: not-allowed;
  filter: saturate(0.6);
}

.toolbar-btn.split-toggle.running {
  background: rgba(106, 169, 255, 0.14);
}

.toolbar-btn.stop-action {
  color: #ffb3b3;
  background: rgba(198, 73, 73, 0.12);
}

.toolbar-btn.stop-action:hover {
  background: rgba(198, 73, 73, 0.2);
}

.toolbar-icon {
  font-size: 15px;
  font-weight: 800;
  line-height: 1;
}

.compact-progress {
  font-size: 11px;
  color: var(--text-secondary);
}

.toolbar-btn.split-toggle {
  color: var(--text-secondary);
}

.toolbar-btn.split-toggle.active {
  color: var(--text-primary);
  background: rgba(106, 169, 255, 0.16);
}

.status-chip {
  padding: 5px 10px;
  border-radius: 999px;
  font-size: 11px;
  line-height: 1;
  color: var(--text-primary);
  border: 1px solid var(--line-soft);
  white-space: nowrap;
}

.status-chip.succeeded {
  background: rgba(61, 181, 112, 0.16);
}

.status-chip.running,
.status-chip.pending {
  background: rgba(214, 151, 44, 0.18);
}

.status-chip.failed {
  background: rgba(198, 73, 73, 0.18);
}

.status-chip.indexed {
  background: rgba(61, 181, 112, 0.16);
}

.status-chip.indexing,
.status-chip.pending {
  background: rgba(214, 151, 44, 0.18);
}

.status-chip.stale {
  background: rgba(198, 73, 73, 0.18);
}

.pdf-toolbar-controls {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: var(--text-secondary);
  font-size: 12px;
}

.toolbar-control-group {
  display: inline-flex;
  align-items: center;
  overflow: hidden;
}

.pdf-toolbar-controls button {
  width: 32px;
  height: 30px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  padding: 0;
  font-size: 17px;
  font-weight: 750;
}

.page-meter,
.zoom-meter {
  min-width: 52px;
  text-align: center;
  white-space: nowrap;
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 650;
}

.translation-error {
  margin: 14px 20px 0;
  border: 1px solid rgba(198, 73, 73, 0.28);
  background: rgba(198, 73, 73, 0.12);
  color: #ffb3b3;
  border-radius: 12px;
  padding: 12px 14px;
  font-size: 13px;
}

.viewer-stage {
  flex: 1;
  min-height: 0;
  padding: 0;
}

.empty-stage,
.viewer-card {
  height: 100%;
  border: 1px solid var(--line-soft);
  border-radius: 18px;
  background: var(--bg-panel);
  overflow: visible;
}

.empty-stage {
  display: grid;
  place-content: center;
  gap: 12px;
  text-align: center;
}

.empty-title {
  color: var(--text-primary);
  font-size: 18px;
  font-weight: 700;
}

.empty-copy {
  color: var(--text-secondary);
  font-size: 13px;
}

.progress-bar {
  width: 320px;
  max-width: 70vw;
  height: 10px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.06);
  overflow: hidden;
}

.progress-bar span {
  display: block;
  height: 100%;
  background: linear-gradient(90deg, #6aa9ff, #8ae8ff);
}

.compare-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
  height: 100%;
}

.local-pdf-layout {
  position: relative;
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 0;
  height: 100%;
  min-height: 0;
}

.local-pdf-layout.is-dual {
  grid-template-columns: minmax(0, 1fr) minmax(320px, 1fr);
  gap: 14px;
}

.local-pdf-layout.is-translation-only {
  grid-template-columns: minmax(0, 1fr);
}

.translated-single {
  height: 100%;
  display: grid;
  grid-template-columns: minmax(0, 1fr);
}

.single-view {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 0;
  height: 100%;
}

.viewer-card-header {
  position: relative;
  min-height: 48px;
  border-bottom: 1px solid var(--line-soft);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 16px;
  color: var(--text-secondary);
  font-size: 12px;
}

.real-pdf-viewer {
  min-width: 0;
}

.pdf-viewer-loading {
  flex: 1;
  min-width: 0;
  min-height: 0;
  margin: 8px;
  border-radius: 6px;
  border: 1px solid rgba(255, 255, 255, 0.06);
  background:
    linear-gradient(90deg, transparent, rgba(255, 255, 255, 0.055), transparent),
    rgba(255, 255, 255, 0.025);
  background-size: 220% 100%;
  animation: pdf-viewer-loading 1.25s ease-in-out infinite;
}

@keyframes pdf-viewer-loading {
  0% {
    background-position: 180% 0;
  }

  100% {
    background-position: -80% 0;
  }
}

.local-pdf-source {
  min-height: 0;
}

.local-pdf-source > .real-pdf-viewer {
  flex: 1;
  min-height: 0;
}

.local-pdf-layout:not(.is-dual) .local-pdf-source {
  border: 0;
  border-radius: 0;
  background: transparent;
}

.pane-link-rail {
  position: absolute;
  top: 54px;
  left: 50%;
  z-index: 8;
  transform: translateX(-50%);
  pointer-events: none;
}

.pane-link-button {
  width: 28px;
  height: 28px;
  display: grid;
  place-items: center;
  border: 1px solid var(--line-soft);
  border-radius: 9px;
  background: rgba(18, 23, 31, 0.86);
  color: var(--text-muted);
  cursor: pointer;
  pointer-events: auto;
  backdrop-filter: blur(8px);
  transition:
    border-color 160ms ease,
    background 160ms ease,
    color 160ms ease,
    box-shadow 160ms ease;
}

.pane-link-button svg {
  width: 17px;
  height: 17px;
}

.pane-link-button.linked {
  border-color: rgba(106, 169, 255, 0.52);
  background: rgba(37, 99, 235, 0.2);
  color: #bfdbfe;
  box-shadow: 0 0 0 1px rgba(106, 169, 255, 0.12);
}

.pane-link-button:disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

.pdf-compare-pane,
.translated-reader {
  min-height: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.compare-pdf-viewer {
  flex: 1;
}

.translated-artifact-viewer {
  flex: 1;
  min-height: 0;
}

.translation-page {
  flex: 1;
  min-height: 0;
  margin: 6px 8px 8px;
  padding: 0;
  overflow: auto;
  border-radius: 6px;
  background: #f4f1e8;
  color: #1d2126;
  box-shadow: none;
}

/* Shape comes from the global scrollbar style; only the thumb COLOR is
   overridden here — a white thumb would be invisible on this cream "paper"
   background, so use a dark one (faint by default, deeper on hover/scroll). */
.translation-page::-webkit-scrollbar-thumb {
  background-color: rgba(29, 33, 38, 0.12);
}

.translation-page:hover::-webkit-scrollbar-thumb,
.translation-page.is-scrolling::-webkit-scrollbar-thumb {
  background-color: rgba(29, 33, 38, 0.28);
}

.translation-page.wide {
  max-width: none;
  width: calc(100% - 16px);
  margin: 6px 8px 8px;
}

.translation-page.has-full-state {
  display: grid;
  place-items: center;
  background: #171b22;
  color: var(--text-primary);
}

.translation-full-state {
  width: min(360px, calc(100% - 48px));
  display: grid;
  justify-items: center;
  gap: 14px;
  text-align: center;
  color: var(--text-primary);
}

.translation-full-logo {
  width: 72px;
  height: 72px;
  display: grid;
  place-items: center;
  border-radius: 18px;
  border: 1px solid rgba(106, 169, 255, 0.38);
  background: rgba(255, 255, 255, 0.06);
  box-shadow: 0 18px 38px rgba(0, 0, 0, 0.28);
}

.translation-full-logo img {
  width: 54px;
  height: 54px;
  object-fit: contain;
}

.translation-full-title {
  font-size: 18px;
  font-weight: 800;
}

.translation-full-detail {
  max-width: 460px;
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 1.7;
}

.translation-full-track {
  width: min(220px, 70%);
  height: 3px;
  overflow: hidden;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.12);
}

.translation-full-track span {
  display: block;
  height: 100%;
  min-width: 18%;
  border-radius: inherit;
  background: linear-gradient(90deg, #38bdf8, #2563eb);
  transition: width 180ms ease;
}

.translation-full-state.loading .translation-full-track span {
  animation: translation-loading-sweep 1.25s ease-in-out infinite;
}

@keyframes translation-loading-sweep {
  0% {
    transform: translateX(-110%);
  }

  100% {
    transform: translateX(520%);
  }
}

.translation-full-state.error .translation-full-logo {
  border-color: rgba(248, 113, 113, 0.42);
}

.translation-full-state.error .translation-full-title {
  color: #fca5a5;
}
.translation-full-state.info .translation-full-logo {
  border-color: rgba(106, 169, 255, 0.34);
}
.translation-full-state.info .translation-full-title {
  color: var(--text-secondary);
}

.translation-full-retry {
  border: 1px solid rgba(106, 169, 255, 0.32);
  border-radius: 6px;
  padding: 7px 12px;
  background: rgba(106, 169, 255, 0.12);
  color: #bfdbfe;
  cursor: pointer;
}

.translation-full-retry:hover {
  background: rgba(106, 169, 255, 0.18);
}

.translation-page-sheet {
  min-height: 0;
  margin: 0;
  padding: 12px 14px 18px;
  border-radius: 0;
  background: transparent;
}

.translation-page-sheet.active {
  outline: 1px solid rgba(59, 130, 246, 0.28);
  outline-offset: -1px;
}

.translation-page-sheet.pending {
  min-height: 116px;
}

.translation-page-sheet.pending.empty {
  display: flex;
  align-items: center;
}

.translation-page-sheet.failed {
  background: rgba(180, 58, 42, 0.08);
}

.translation-placeholder {
  padding: 28px 0;
  color: #64748b;
  font-size: 13px;
  text-align: center;
}

.translation-page-content {
  padding: 28px 30px 34px;
  border-radius: 10px;
  background: #f3efe7;
  color: #1d2126;
  box-shadow: 0 16px 42px rgba(0, 0, 0, 0.16);
  max-width: 860px;
  margin: 0 auto;
}

.translation-page-title {
  margin-bottom: 18px;
  font-size: 17px;
  font-weight: 700;
}

.translation-page-block {
  display: block;
  width: 100%;
  text-align: left;
  border: 1px solid rgba(117, 126, 140, 0.2);
  background: rgba(255, 255, 255, 0.54);
  color: inherit;
  border-radius: 8px;
  padding: 14px 16px;
  margin-bottom: 12px;
  cursor: pointer;
}

.translation-page-block.active {
  border-color: rgba(59, 130, 246, 0.56);
  background: rgba(232, 240, 255, 0.96);
}

.translation-page-block-source {
  font-size: 12px;
  font-weight: 700;
  color: #4b5563;
  margin-bottom: 8px;
}

.translation-page-block-translation {
  font-size: 13px;
  line-height: 1.7;
  white-space: pre-wrap;
}

.page-card {
  position: relative;
  max-width: 780px;
  min-height: 620px;
  margin: 20px auto;
  background: #f3efe7;
  color: #1d2126;
  border-radius: 8px;
  box-shadow: 0 20px 50px rgba(0, 0, 0, 0.22);
  padding: 36px 42px 70px;
  overflow: visible;
}

.page-card.translated {
  background: #ece8e0;
}

.compare-grid .page-card {
  max-width: none;
  min-height: 560px;
  padding: 30px 34px 54px;
}

.compare-grid .text-block {
  font-size: 13px;
  line-height: 1.72;
}

.page-heading {
  font-size: 18px;
  font-weight: 700;
  margin-bottom: 24px;
}

.text-block {
  width: 100%;
  display: block;
  text-align: left;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 8px;
  padding: 10px 12px;
  line-height: 1.8;
  font-size: 15px;
  color: inherit;
  margin-bottom: 8px;
  cursor: pointer;
}

.text-block.active {
  background: rgba(250, 204, 21, 0.24);
  border-color: rgba(214, 164, 20, 0.35);
}

.selection-toolbar {
  position: absolute;
  left: 50%;
  bottom: 18px;
  transform: translateX(-50%);
  display: flex;
  gap: 8px;
  background: #1e2228;
  padding: 8px;
  border-radius: 14px;
  box-shadow: 0 12px 28px rgba(0, 0, 0, 0.28);
}

.selection-toolbar button {
  min-height: 34px;
  min-width: 80px;
  border-radius: 10px;
  border: none;
  background: transparent;
  color: #d6dce6;
  cursor: pointer;
}

.selection-toolbar .active-action {
  background: rgba(106, 169, 255, 0.18);
}

.inline-translation {
  position: absolute;
  right: 28px;
  bottom: 92px;
  width: 360px;
  border-radius: 14px;
  border: 1px solid rgba(106, 169, 255, 0.28);
  background: rgba(255, 255, 255, 0.98);
  color: #111827;
  padding: 14px;
  box-shadow: 0 18px 40px rgba(15, 23, 42, 0.18);
  z-index: 6;
}

.inline-translation-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  font-size: 13px;
  font-weight: 700;
}

.inline-translation-head button,
.toggle-source {
  border: none;
  background: transparent;
  color: #2563eb;
  cursor: pointer;
}

.inline-translation-body,
.inline-source {
  margin-top: 12px;
  line-height: 1.65;
  font-size: 13px;
}

.inline-source {
  padding-top: 10px;
  border-top: 1px solid #dbe2ee;
  color: #5f6a7c;
}

.page-nav {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.page-chip {
  min-width: 58px;
  min-height: 38px;
  border-radius: 12px;
  border: 1px solid var(--line-soft);
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-secondary);
  cursor: pointer;
}

.page-chip.active {
  background: rgba(106, 169, 255, 0.12);
  color: var(--text-primary);
}
</style>
