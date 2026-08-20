<script setup>
import { reactive, ref } from 'vue'
import ReaderPane from '../components/ReaderPane.vue'
import { translationLanguages } from '../mockData'
import { messages } from '../i18n'
import { normalizeLinkedBlockHover } from '../translationLinking'

const ui = messages.en
const translationLang = ref('ko')
const viewMode = ref('dual')
const activePage = ref(1)
const activeBlockId = ref('')
const activeHighlight = ref(null)
const hoveredLinkedBlock = ref(null)
const requestedPages = ref([])

const document = reactive({
  id: 'e2e-pdf',
  source: 'local',
  title: 'E2E Translation Linking.pdf',
  shortTitle: 'E2E.pdf',
  status: 'indexed',
  statusTone: 'success',
  treeReady: true,
  lastOpened: { en: 'E2E', zh: 'E2E' },
  pageCount: 10,
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
  chatModelId: '',
  quoteBlockId: '',
  chatReady: true,
  translation: {
    status: 'succeeded',
    progress: 5,
    total: 7,
    failedBlocks: 0,
    lang: 'zh',
    error: '',
    jobId: 'e2e',
    providerKey: 'e2e',
    phase: 'finished',
    currentPage: 1,
    pages: {
      1: {
        page: 1,
        status: 'succeeded',
        cachedBlocks: 5,
        failedBlocks: 0,
        totalBlocks: 5,
        blocks: [
          {
            blockId: 'p1-caption-label',
            sourceText: 'Figure 3',
            translatedText: '图3',
            status: 'succeeded',
            blockRole: 'caption',
            bboxList: [[0.1, 0.1, 0.18, 0.18]],
          },
          {
            blockId: 'p1-caption-tail',
            sourceText: 'demonstrates how SWE-Pruner functions as middleware.',
            translatedText: '演示了 SWE-Pruner 如何作为中间件工作。',
            status: 'succeeded',
            blockRole: 'body',
            bboxList: [[0.6, 0.1, 0.86, 0.18], [0.1, 0.19, 0.72, 0.27]],
          },
          {
            blockId: 'p1-caption-title',
            sourceText: 'Overview of SWE-Pruner. Left:',
            translatedText: 'SWE-Pruner 概述。左：',
            status: 'succeeded',
            blockRole: 'body',
            bboxList: [[0.2, 0.1, 0.42, 0.18]],
          },
          {
            blockId: 'p1-caption-subject',
            sourceText: 'The Interaction Workflow',
            translatedText: '交互工作流程',
            status: 'succeeded',
            blockRole: 'body',
            bboxList: [[0.44, 0.1, 0.58, 0.18]],
          },
          {
            blockId: 'p1-b2',
            sourceText: 'Neural Skimmer',
            translatedText: '轻量级神经裁剪器',
            status: 'succeeded',
            blockRole: 'body',
            bboxList: [[0.1, 0.36, 0.72, 0.44]],
          },
        ],
      },
      3: {
        page: 3,
        status: 'succeeded',
        cachedBlocks: 0,
        failedBlocks: 0,
        totalBlocks: 0,
        blocks: [],
      },
    },
  },
  pages: [],
  messages: [],
})

function createPageTwoTranslation() {
  return {
    page: 2,
    status: 'succeeded',
    cachedBlocks: 2,
    failedBlocks: 0,
    totalBlocks: 2,
    blocks: [
      {
        blockId: 'p2-b1',
        sourceText: 'Continuous Translation Reader',
        translatedText: '连续译文阅读器',
        status: 'succeeded',
        blockRole: 'heading',
        bboxList: [[0.1, 0.36, 0.72, 0.44]],
      },
      {
        blockId: 'p2-b2',
        sourceText: 'Lazy Page Loading',
        translatedText: '按页懒加载',
        status: 'succeeded',
        blockRole: 'body',
        bboxList: [[0.1, 0.48, 0.72, 0.56]],
      },
    ],
  }
}

function createHighlight(source) {
  return {
    page: source.page,
    bboxList: source.bboxList || [],
  }
}

function setPage(page, blockId = '', source = null) {
  activePage.value = page
  if (source) hoveredLinkedBlock.value = null
  if (blockId) activeBlockId.value = blockId
  if (source?.bboxList?.length) {
    activeHighlight.value = createHighlight({
      page,
      bboxList: source.bboxList,
    })
  } else if (source?.clearHighlight) {
    activeHighlight.value = null
  }
}

function setLinkedBlockHover(block = null) {
  hoveredLinkedBlock.value = normalizeLinkedBlockHover(block)
}

function updateDocumentPageCount(payload) {
  document.pageCount = Math.max(document.pageCount, Number(payload.pageCount || 0))
}

function requestTranslationPage(pageNo) {
  const normalizedPage = Number(pageNo)
  requestedPages.value = [...requestedPages.value, normalizedPage]
  if (normalizedPage !== 2 || document.translation.pages[2]) return
  document.translation.pages[2] = createPageTwoTranslation()
  document.translation.progress = 7
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
      :active-block-id="activeBlockId"
      :active-highlight="activeHighlight"
      :hovered-linked-block="hoveredLinkedBlock"
      :active-translation="null"
      :page-translation="document.translation.pages[activePage] || null"
      :selection-locked="false"
      :inline-translate-open="false"
      locale="en"
      :ui="ui"
      @update:translationLang="translationLang = $event"
      @set-view-mode="viewMode = $event"
      @select-page="setPage"
      @linked-block-hover="setLinkedBlockHover"
      @request-translation-page="requestTranslationPage"
      @document-loaded="updateDocumentPageCount"
    />
  </div>
  <output data-testid="active-page">{{ activePage }}</output>
  <output data-testid="requested-translation-pages">{{ requestedPages.join(',') }}</output>
</template>

<style scoped>
.e2e-reader-shell {
  display: flex;
  height: 520px;
  min-height: 0;
}

.e2e-reader-shell :deep(.reader-pane) {
  height: 100%;
  min-height: 0;
}
</style>
