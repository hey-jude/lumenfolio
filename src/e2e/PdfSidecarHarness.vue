<script setup>
import { computed, reactive, ref } from 'vue'
import ReaderPane from '../components/ReaderPane.vue'
import { translationLanguages } from '../mockData'
import { messages } from '../i18n'

const ui = messages.en
const params = new URLSearchParams(window.location.search)
const scenario = params.get('scenario') || 'complete'
const translationLang = ref('zh')
const viewMode = ref('original')
const activePage = ref(1)
const startCalls = ref(0)
const cancelCalls = ref(0)
const loadedArtifacts = ref([])
const partialHistory = ref([])
const lastTranslationContext = ref(null)

const document = reactive({
  id: 'e2e-pdf-sidecar',
  source: 'local',
  title: 'E2E PDF Sidecar.pdf',
  shortTitle: 'PDF Sidecar.pdf',
  status: 'indexed',
  statusTone: 'success',
  treeReady: true,
  pageCount: 3,
  indexVersion: 11,
  currentIndexVersion: 11,
  indexStatus: 'indexed',
  indexProgress: {
    percent: 100,
    stage: 'complete',
    label: '',
  },
  visualIndexStatus: 'pending',
  visualIndexVersion: 0,
  visualIndexError: '',
  currentPage: 1,
  chatReady: true,
  translation: {
    status: 'idle',
    progress: 0,
    total: 0,
    failedBlocks: 0,
    lang: 'zh',
    error: '',
    jobId: '',
    pdfJobId: '',
    providerKey: '',
    phase: '',
    currentPage: 1,
    pdfStatus: 'idle',
    pdfProgressPercent: 0,
    monoPdfPath: '',
    dualPdfPath: '',
    pdfArtifactScope: '',
    pdfArtifactPages: '',
    partialArtifacts: {},
    cached: false,
    pages: {},
  },
  pages: [],
  messages: [],
})

const statusSummary = computed(() => [
  document.translation.status,
  document.translation.phase,
  document.translation.pdfArtifactScope,
  document.translation.pdfArtifactPages,
].filter(Boolean).join('|'))

function applyPdfJob(payload) {
  if (payload.artifactScope === 'partial' && payload.artifactPages) {
    partialHistory.value = [...partialHistory.value, String(payload.artifactPages)]
  }
  document.translation.jobId = payload.jobId || document.translation.jobId
  document.translation.pdfJobId = payload.jobId || document.translation.pdfJobId
  document.translation.status = payload.status || document.translation.status
  document.translation.pdfStatus = payload.status || document.translation.pdfStatus
  document.translation.progress = Number(payload.progressPercent || 0)
  document.translation.total = 100
  document.translation.phase = payload.phase || document.translation.phase
  document.translation.currentPage = Number(payload.currentPage || 1)
  document.translation.pdfProgressPercent = Number(payload.progressPercent || 0)
  document.translation.monoPdfPath = payload.monoPdfPath || document.translation.monoPdfPath
  document.translation.dualPdfPath = payload.dualPdfPath || document.translation.dualPdfPath
  document.translation.pdfArtifactScope = payload.artifactScope || document.translation.pdfArtifactScope
  document.translation.pdfArtifactPages = payload.artifactPages || document.translation.pdfArtifactPages
  if (payload.artifactScope === 'partial' && payload.monoPdfPath) {
    const pages = String(payload.artifactPages || payload.currentPage || '')
      .split(',')
      .map((page) => Number(page.trim()))
      .filter((page) => page > 0)
    const nextArtifacts = { ...(document.translation.partialArtifacts || {}) }
    for (const page of pages) {
      nextArtifacts[page] = {
        monoPdfPath: payload.monoPdfPath,
        dualPdfPath: payload.dualPdfPath || '',
        artifactPages: payload.artifactPages || String(page),
      }
    }
    document.translation.partialArtifacts = nextArtifacts
  }
  document.translation.cached = Boolean(payload.cached)
  document.translation.error = payload.error || ''
}

function startPdfTranslation(context = {}) {
  lastTranslationContext.value = context
  startCalls.value += 1
  viewMode.value = 'dual'
  if (scenario === 'error-retry' && startCalls.value === 1) {
    applyPdfJob({
      jobId: `pdf-job-${startCalls.value}`,
      status: 'failed',
      phase: 'failed',
      progressPercent: 0,
      error: 'Unsupported PDF translation provider',
    })
    return
  }
  applyPdfJob({
    jobId: `pdf-job-${startCalls.value}`,
    status: 'running',
    phase: 'starting',
    progressPercent: 1,
    currentPage: 1,
  })
  window.setTimeout(() => {
    if (document.translation.status === 'canceled') return
    applyPdfJob({
      jobId: document.translation.pdfJobId,
      status: 'partial',
      phase: 'partial_ready',
      progressPercent: 45,
      currentPage: 1,
      monoPdfPath: '/tmp/lumenfolio/e2e.partial.mono.pdf',
      dualPdfPath: '/tmp/lumenfolio/e2e.partial.dual.pdf',
      artifactScope: 'partial',
      artifactPages: '1',
    })
  }, 20)
  if (scenario === 'multi-partial') {
    window.setTimeout(() => {
      if (document.translation.status === 'canceled') return
      applyPdfJob({
        jobId: document.translation.pdfJobId,
        status: 'partial',
        phase: 'partial_ready',
        progressPercent: 55,
        currentPage: 2,
        monoPdfPath: '/tmp/lumenfolio/e2e.partial.p2.mono.pdf',
        dualPdfPath: '/tmp/lumenfolio/e2e.partial.p2.dual.pdf',
        artifactScope: 'partial',
        artifactPages: '2',
      })
    }, 80)
  }
  if (scenario === 'complete' || scenario === 'error-retry') {
    window.setTimeout(() => {
      if (document.translation.status === 'canceled') return
      applyPdfJob({
        jobId: document.translation.pdfJobId,
        status: 'succeeded',
        phase: 'finished',
        progressPercent: 100,
        monoPdfPath: '/tmp/lumenfolio/e2e.full.mono.pdf',
        dualPdfPath: '/tmp/lumenfolio/e2e.full.dual.pdf',
        artifactScope: 'full',
        artifactPages: '',
      })
    }, 140)
  }
}

function cancelPdfTranslation() {
  cancelCalls.value += 1
  document.translation.status = 'canceled'
  document.translation.pdfStatus = 'canceled'
  document.translation.phase = 'canceled'
}

function updateDocumentPageCount(payload) {
  document.pageCount = Math.max(document.pageCount, Number(payload.pageCount || 0))
}

function updateArtifactState(payload) {
  if (document.translation.monoPdfPath || document.translation.dualPdfPath) {
    loadedArtifacts.value = [...loadedArtifacts.value, statusSummary.value]
  }
  updateDocumentPageCount(payload)
}
</script>

<template>
  <div class="e2e-reader-shell">
    <ReaderPane
      :document="document"
      :translation-languages="translationLanguages"
      :translation-lang="translationLang"
      :view-mode="viewMode"
      :active-page="activePage"
      active-block-id=""
      :active-highlight="null"
      :hovered-linked-block="null"
      :active-translation="null"
      :page-translation="null"
      :selection-locked="false"
      :inline-translate-open="false"
      locale="en"
      :ui="ui"
      @update:translationLang="translationLang = $event"
      @translation-action="startPdfTranslation"
      @cancel-translation="cancelPdfTranslation"
      @set-view-mode="viewMode = $event"
      @select-page="activePage = $event"
      @document-loaded="updateDocumentPageCount"
    />
  </div>
  <output data-testid="pdf-sidecar-status">{{ statusSummary }}</output>
  <output data-testid="pdf-sidecar-start-calls">{{ startCalls }}</output>
  <output data-testid="pdf-sidecar-cancel-calls">{{ cancelCalls }}</output>
  <output data-testid="pdf-sidecar-artifact-loads">{{ loadedArtifacts.join(',') }}</output>
  <output data-testid="pdf-sidecar-partial-history">{{ partialHistory.join(',') }}</output>
  <output data-testid="pdf-sidecar-translation-context">{{ JSON.stringify(lastTranslationContext) }}</output>
</template>

<style scoped>
.e2e-reader-shell {
  height: min(760px, 100vh);
  min-height: 0;
  display: flex;
}
</style>
