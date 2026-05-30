<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import MarkdownText from './MarkdownText.vue'
import { startWindowDrag } from '../windowDrag'

const props = defineProps({
  document: {
    type: Object,
    required: true,
  },
  collapsed: {
    type: Boolean,
    default: false,
  },
  width: {
    type: Number,
    default: 420,
  },
  activeCitationId: {
    type: String,
    default: '',
  },
  pendingSelection: {
    type: Object,
    default: null,
  },
  focusRequest: {
    type: Number,
    default: 0,
  },
  availableModels: {
    type: Array,
    required: true,
  },
  currentModelId: {
    type: String,
    required: true,
  },
  currentModel: {
    type: Object,
    default: null,
  },
  modelConfigured: {
    type: Boolean,
    default: true,
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
  'toggle-collapse',
  'citation-click',
  'send',
  'update:model-id',
  'clear-selection',
  'clear-history',
])

const chatInputEnabled = computed(() => props.document.chatReady && props.modelConfigured)
const supportsVision = computed(() => props.currentModel?.capabilities?.includes('vision'))
const visibleMessages = computed(() => (props.document.messages || [])
  .filter((message) => message && typeof message === 'object')
  .filter((message) => !String(message.id || '').startsWith(`welcome-${props.document.id}`)))
const hasChatHistory = computed(() => (props.document.messages || [])
  .filter((message) => message && typeof message === 'object')
  .some((message) => !String(message.id || '').startsWith(`welcome-${props.document.id}`)))
const pendingImageDataUrl = ref('')
const pendingImageName = ref('')
const fileInputRef = ref(null)
const composerTextareaRef = ref(null)
const messageListRef = ref(null)
const autoFollowMessages = ref(true)
const userScrolledMessages = ref(false)
const showJumpToLatest = ref(false)
const messageListScrolling = ref(false)
const expandedAgentStages = ref({})
const expandedAgentPanels = ref({})
let messageScrollFrame = 0
let messageScrollbarTimer = null
let lastMessageScrollTop = 0
const messageScrollSettleTimers = new Set()
const capabilityTitle = computed(() => {
  if (!props.modelConfigured) return props.ui.modelNotConfigured
  const labels = [props.ui.capabilityText]
  if (supportsVision.value) labels.push(props.ui.capabilityVision)
  if (props.currentModel?.capabilities?.includes('reasoning')) labels.push(props.ui.capabilityReasoning)
  if (props.currentModel?.capabilities?.includes('tool_use')) labels.push(props.ui.capabilityToolUse)
  return `${props.ui.capabilities}: ${labels.join(' + ')}`
})
const pendingSelectionSource = computed(() => (
  props.pendingSelection?.sourceType === 'paragraph'
    ? props.ui.paragraph
    : props.ui.selectedText
))
const pendingSelectionLabel = computed(() => {
  const page = props.pendingSelection?.page || ''
  return props.locale === 'zh'
    ? `第${page}${props.ui.page} · ${pendingSelectionSource.value}`
    : `${props.ui.page} ${page} · ${pendingSelectionSource.value}`
})
const pendingSelectionPreview = computed(() => {
  const text = String(props.pendingSelection?.text || '').replace(/\s+/g, ' ').trim()
  return text.length > 260 ? `${text.slice(0, 260)}...` : text
})

function modelOptionLabel(model) {
  // Show only the model name. The provider prefix (e.g. "DS · DeepSeek · ")
  // overflowed the trigger and was redundant — model names already imply
  // their provider.
  return model.label
}

function handleSubmit(event) {
  if (!chatInputEnabled.value) return
  const text = String(event.target.querySelector('textarea')?.value || '').trim()
  if (!text && !pendingImageDataUrl.value) return
  emit('send', {
    text: text || props.ui.imageOnlyPrompt,
    imageDataUrl: pendingImageDataUrl.value || '',
    imageName: pendingImageName.value || '',
  })
  event.target.reset()
  clearPendingImage()
  autoFollowMessages.value = true
  scrollMessagesToBottom({ force: true })
}

function focusComposer() {
  nextTick(() => {
    if (props.collapsed) return
    composerTextareaRef.value?.focus()
  })
}

function handleComposerKeydown(event) {
  if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return
  event.preventDefault()
  event.currentTarget?.form?.requestSubmit()
}

function clearPendingImage() {
  pendingImageDataUrl.value = ''
  pendingImageName.value = ''
  if (fileInputRef.value) fileInputRef.value.value = ''
}

function openImagePicker() {
  if (!chatInputEnabled.value || !supportsVision.value) return
  fileInputRef.value?.click()
}

function handleImagePicked(event) {
  const file = event.target.files?.[0]
  acceptImageFile(file)
}

function acceptImageFile(file) {
  if (!chatInputEnabled.value || !supportsVision.value || !file?.type?.startsWith('image/')) return
  const reader = new FileReader()
  reader.onload = () => {
    pendingImageDataUrl.value = typeof reader.result === 'string' ? reader.result : ''
    pendingImageName.value = file.name
  }
  reader.readAsDataURL(file)
}

function handleComposerPaste(event) {
  if (!chatInputEnabled.value || !supportsVision.value) return
  const imageItem = Array.from(event.clipboardData?.items || [])
    .find((item) => item.kind === 'file' && item.type.startsWith('image/'))
  const file = imageItem?.getAsFile()
  if (!file) return
  event.preventDefault()
  acceptImageFile(file)
}

function handleComposerDrop(event) {
  if (!chatInputEnabled.value || !supportsVision.value) return
  const file = Array.from(event.dataTransfer?.files || [])
    .find((item) => item.type.startsWith('image/'))
  if (!file) return
  acceptImageFile(file)
}

function localized(value) {
  if (!value || typeof value !== 'object') return value
  return value[props.locale] || value.en || Object.values(value)[0]
}

function messageText(message) {
  return String(localized(message.content) || '')
}

function reasoningText(message) {
  return String(message.reasoningContent || '').trim()
}

function latestActivityEvent(message) {
  const events = traceActivityEvents(message)
  return events.length ? events[events.length - 1] : null
}

function runningStatusLabel(message) {
  const event = latestActivityEvent(message)
  if (event?.step === 'generate_answer' || eventType(event) === 'answer_start') return props.ui.chatGenerating
  if (event?.step) return agentProcessState(message)
  return props.ui.chatWorking
}

function isWaitingForAnswer(message) {
  return message.role === 'assistant' && message.status === 'running' && !messageText(message).trim()
}

function messageListAtBottom() {
  const element = messageListRef.value
  if (!element) return true
  const distance = element.scrollHeight - element.scrollTop - element.clientHeight
  return distance <= 64
}

function updateMessageScrollState() {
  const element = messageListRef.value
  if (!element) return

  messageListScrolling.value = true
  if (messageScrollbarTimer) window.clearTimeout(messageScrollbarTimer)
  messageScrollbarTimer = window.setTimeout(() => {
    messageListScrolling.value = false
    messageScrollbarTimer = null
  }, 760)

  const scrollTop = element.scrollTop
  const atBottom = messageListAtBottom()
  // Direction-aware stick-to-bottom: an actual upward scroll detaches
  // auto-follow so streaming output never yanks the view back down. Content
  // growth during streaming keeps scrollTop unchanged (only scrollHeight
  // grows), so it can't be mistaken for a user scrolling up.
  if (scrollTop < lastMessageScrollTop - 1 && !atBottom) {
    autoFollowMessages.value = false
    userScrolledMessages.value = true
  } else if (atBottom) {
    // Returning to the bottom re-attaches auto-follow.
    autoFollowMessages.value = true
    userScrolledMessages.value = false
  }
  lastMessageScrollTop = scrollTop
  showJumpToLatest.value = !atBottom && visibleMessages.value.length > 0
}

function markUserScrolledMessages() {
  // A manual gesture (wheel/touch/pointer/key) while not pinned to the bottom
  // detaches auto-follow immediately. The scroll handler then refines the
  // state using scroll position/direction.
  if (!messageListAtBottom()) {
    autoFollowMessages.value = false
    userScrolledMessages.value = true
  }
}

function scrollMessagesToBottom({ force = false, smooth = false, settle = false } = {}) {
  if (!force && !autoFollowMessages.value) return
  if (messageScrollFrame) window.cancelAnimationFrame(messageScrollFrame)
  nextTick(() => {
    messageScrollFrame = window.requestAnimationFrame(() => {
      messageScrollFrame = 0
      const element = messageListRef.value
      if (!element) return
      element.scrollTo({
        top: element.scrollHeight,
        behavior: smooth ? 'smooth' : 'auto',
      })
      autoFollowMessages.value = true
      userScrolledMessages.value = false
      showJumpToLatest.value = false
    })
  })
  if (!settle) return
  for (const delay of [60, 180, 360]) {
    const timer = window.setTimeout(() => {
      messageScrollSettleTimers.delete(timer)
      // Re-evaluate follow state at fire time and never force: a user who
      // scrolled up between the initial scroll and this delayed settle pass
      // must not be dragged back to the bottom. This was the core bug.
      if (autoFollowMessages.value) scrollMessagesToBottom({ smooth: false })
    }, delay)
    messageScrollSettleTimers.add(timer)
  }
}

function jumpToLatestMessage() {
  autoFollowMessages.value = true
  userScrolledMessages.value = false
  scrollMessagesToBottom({ force: true, smooth: true })
}

watch(supportsVision, (enabled) => {
  if (!enabled) clearPendingImage()
})

watch(() => visibleMessages.value.map((message) => [
  message.id,
  message.status,
  messageText(message).length,
  reasoningText(message).length,
  traceActivityEvents(message).length,
  message.citations?.length || 0,
].join(':')).join('|'), () => {
  scrollMessagesToBottom({ force: !userScrolledMessages.value, settle: !userScrolledMessages.value })
}, { flush: 'post' })

watch(() => props.document.id, () => {
  autoFollowMessages.value = true
  userScrolledMessages.value = false
  showJumpToLatest.value = false
  scrollMessagesToBottom({ force: true, settle: true })
})

watch(() => props.document.chatHistoryLoaded, (loaded) => {
  if (!loaded) return
  scrollMessagesToBottom({ force: !userScrolledMessages.value, settle: !userScrolledMessages.value })
}, { flush: 'post' })

watch(() => props.focusRequest, (request) => {
  if (!request) return
  focusComposer()
})

watch(() => props.collapsed, (collapsed) => {
  if (!collapsed && props.focusRequest) focusComposer()
})

onMounted(() => {
  if (props.focusRequest) focusComposer()
  autoFollowMessages.value = true
  userScrolledMessages.value = false
  scrollMessagesToBottom({ force: true, settle: true })
})

onBeforeUnmount(() => {
  if (messageScrollFrame) window.cancelAnimationFrame(messageScrollFrame)
  if (messageScrollbarTimer) window.clearTimeout(messageScrollbarTimer)
  messageScrollSettleTimers.forEach((timer) => window.clearTimeout(timer))
  messageScrollSettleTimers.clear()
})

defineExpose({
  focusComposer,
})

function formatTraceStep(step) {
  const labels = {
    session_compact: 'Prepare context',
    intent_classify: 'Understand question',
    inspect_tree: 'Inspect document structure',
    read_tree_node_lines: 'Read section lines',
    open_section: 'Open relevant sections',
    search_chunks: 'Search document text',
    open_pages: 'Read page overview',
    inspect_tables: 'Inspect tables',
    open_table: 'Open table facts',
    search_table_facts: 'Search table facts',
    inspect_visuals: 'Inspect visuals',
    open_visual: 'Open visual',
    analyze_visual: 'Analyze visual',
    finalize_answer: 'Check evidence',
    generate_answer: 'Generate answer',
    start: 'Prepare document',
  }
  const zhLabels = {
    session_compact: '整理上下文',
    intent_classify: '理解问题',
    inspect_tree: '查看文档结构',
    read_tree_node_lines: '读取章节行文',
    open_section: '打开相关章节',
    search_chunks: '搜索正文片段',
    open_pages: '查看页面概览',
    inspect_tables: '查看表格',
    open_table: '打开表格事实',
    search_table_facts: '搜索表格事实',
    inspect_visuals: '查看图片图表',
    open_visual: '打开视觉证据',
    analyze_visual: '分析视觉证据',
    finalize_answer: '判断证据',
    generate_answer: '生成回答',
    start: '准备文档',
  }
  return props.locale === 'zh'
    ? (zhLabels[step] || step)
    : (labels[step] || step)
}

function formatTraceStatus(status) {
  if (status === 'running') return props.ui.chatWorking
  if (status === 'error') return props.locale === 'zh' ? '失败' : 'Error'
  if (status === 'pending') return props.locale === 'zh' ? '待处理' : 'pending'
  if (status === 'skipped') return props.ui.traceStepSkipped
  return props.ui.traceStepCompleted
}

function traceStatusLabel(trace) {
  const status = trace?.finalizeGate?.status || ''
  if (status === 'answerable') return props.ui.traceStatusAnswerable
  if (status === 'insufficient') return props.ui.traceStatusInsufficient
  if (status === 'needs_more_evidence') return props.ui.traceStatusSearching
  return status || props.ui.traceStatusUnknown
}

function traceAttemptLabel(trace) {
  const gate = trace?.finalizeGate || {}
  const attempt = Number(gate.attempt ?? 0) + 1
  const maxAttempts = Number(gate.maxAttempts || 20)
  return props.ui.traceAttemptSummary
    .replace('{attempt}', String(attempt))
    .replace('{max}', String(maxAttempts))
}

function agentActivitySummary(message) {
  if (!message.retrievalTrace) return runningStatusLabel(message)
  return `${traceStatusLabel(message.retrievalTrace)} · ${traceAttemptLabel(message.retrievalTrace)}`
}

function traceActivityEvents(message) {
  return message.activityEvents?.length ? message.activityEvents : (message.retrievalTrace?.events || [])
}

// Actual retrieval tools only. `session_compact` / `intent_classify` are
// pipeline meta-steps (not tool calls); they get their own row kind so they are
// not mislabeled as "Tool call · 0 results".
const AGENT_TOOL_NAMES = new Set([
  'inspect_tree',
  'read_tree_node_lines',
  'open_section',
  'search_chunks',
  'open_pages',
  'inspect_tables',
  'open_table',
  'search_table_facts',
  'resolve_table_anchor',
  'inspect_visuals',
  'open_visual',
  'analyze_visual',
  'analyze_page',
  'resolve_visual_anchor',
  'cross_document_search',
  'expand_related_documents',
])

// Pipeline meta-steps: real work, but not retrieval tool calls.
const AGENT_META_STEPS = new Set(['session_compact', 'intent_classify'])

// Distinguish the M3 rule gate from the M4 LLM judge so both stay visible.
// M3 emits `judge_result` titled "M3 …"; the M4 loop emits `judge_start` /
// `judge_stop` (M4-only) and `judge_result` titled "M4 …".
function judgeRowKey(event) {
  const type = eventType(event)
  const title = String(event?.title || '')
  if (type === 'judge_start' || type === 'judge_stop' || title.includes('M4')) return 'judge:m4'
  if (title.includes('M3')) return 'judge:m3'
  return 'judge'
}

function agentEventRowKey(event) {
  const type = eventType(event)
  const tool = eventToolName(event)
  const step = String(event?.step || '')
  if (type === 'start') return 'start'
  if (tool === 'current_view') return 'current_view'
  if (AGENT_TOOL_NAMES.has(tool)) return `tool:${tool}`
  if (AGENT_META_STEPS.has(step)) return `meta:${step}`
  if (step === 'finalize_answer' || ['judge_start', 'judge_result', 'judge_stop'].includes(type)) {
    return judgeRowKey(event)
  }
  if (type === 'answer_start' || step === 'generate_answer') return 'answer'
  if (type === 'error') return 'error'
  return ''
}

function createAgentRow(key, event, index) {
  const tool = eventToolName(event)
  const step = String(event?.step || '')
  const isTool = key.startsWith('tool:')
  const isJudge = key.startsWith('judge')
  const isMeta = key.startsWith('meta:')
  const title = isTool ? formatTraceStep(tool) : (isMeta ? formatTraceStep(step) : eventTitle(event))
  return {
    id: key,
    index,
    title,
    subtitle: agentRowSubtitle(key, event),
    status: normalizeAgentRowStatus(event?.status || 'completed'),
    chips: [],
    details: [],
    calls: isTool ? 0 : 1,
    results: 0,
    latestEvent: event,
    isTool,
    isJudge,
    isMeta,
  }
}

function normalizeAgentRowStatus(status) {
  if (status === 'error') return 'error'
  if (status === 'running') return 'running'
  if (status === 'skipped') return 'skipped'
  return 'completed'
}

function agentRowSubtitle(key, event) {
  if (key === 'start') return props.locale === 'zh' ? '准备本地检索和证据判断' : 'Prepare retrieval and evidence checks'
  if (key === 'current_view') return props.locale === 'zh' ? '当前页相关性判断' : 'Current page relevance'
  if (key === 'judge:m3') return props.locale === 'zh' ? '规则判断证据是否足够' : 'Rule check: is evidence sufficient'
  if (key === 'judge:m4') return props.locale === 'zh' ? 'LLM 判断证据是否足够' : 'LLM check: is evidence sufficient'
  if (key.startsWith('judge')) return props.locale === 'zh' ? '判断证据是否足够' : 'Decide evidence sufficiency'
  if (key === 'meta:session_compact') return props.locale === 'zh' ? '整理会话上下文' : 'Compact session context'
  if (key === 'meta:intent_classify') return props.locale === 'zh' ? '识别问题意图' : 'Classify question intent'
  if (key === 'answer') return props.locale === 'zh' ? '基于证据生成回答' : 'Generate answer from evidence'
  if (key === 'error') return props.locale === 'zh' ? '执行失败' : 'Execution failed'
  if (key.startsWith('tool:')) return props.locale === 'zh' ? '工具调用' : 'Tool call'
  return eventSummary(event)
}

function mergeAgentRowEvent(row, event) {
  row.latestEvent = event
  row.status = normalizeAgentRowStatus(event?.status || row.status)
  if (row.isTool && eventType(event) === 'tool_call') row.calls += 1
  const count = toolResultCount(event)
  if (count !== null) row.results += count
  const summary = eventSummary(event)
  if (summary && !row.details.includes(summary)) row.details.push(summary)
}

function finalizeAgentRow(row, message) {
  const chips = []
  if (row.isTool) {
    const calls = row.calls || (row.results ? 1 : 0)
    if (calls) chips.push(props.locale === 'zh' ? `${calls} 次调用` : `${calls} call${calls > 1 ? 's' : ''}`)
    chips.push(`${row.results} ${props.locale === 'zh' ? '结果' : 'results'}`)
  }
  if (row.isJudge) {
    // Use this row's OWN latest verdict (each judge event carries its decision
    // payload) so the M3 row shows the M3 verdict and the M4 row shows the M4
    // verdict — instead of stamping every judge row with the final gate status.
    const ownJudge = row.latestEvent?.judge && typeof row.latestEvent.judge === 'object'
      ? row.latestEvent.judge
      : null
    const status = String(ownJudge?.status || traceGateStatus(message) || '')
    if (status) chips.push(formatJudgeStatus(status))
    const citationCount = Number(ownJudge?.citationCount)
    if (Number.isFinite(citationCount) && citationCount > 0) {
      chips.push(`${citationCount} citations`)
    }
    const reason = String(ownJudge?.reason || '')
    if (reason && !row.details.includes(reason)) row.details.push(reason)
    const missing = Array.isArray(ownJudge?.missing)
      ? ownJudge.missing.map((item) => String(item)).filter(Boolean)
      : []
    if (missing.length) row.details.push(`${props.locale === 'zh' ? '缺少' : 'Missing'}: ${missing.join(', ')}`)
  }
  if (row.id === 'current_view') {
    const count = toolResultCount(row.latestEvent)
    if (count !== null) chips.push(`${count} ${props.locale === 'zh' ? '命中' : 'hits'}`)
  }
  if (row.id === 'answer' && messageText(message).trim()) chips.push(props.locale === 'zh' ? '已生成' : 'written')
  if (!row.details.length) {
    const summary = eventSummary(row.latestEvent)
    if (summary) row.details.push(summary)
  }
  if (row.id === 'start') row.details = []
  row.chips = chips.slice(0, 3)
  row.details = row.details.filter(Boolean).slice(0, 4)
  return row
}

function agentTimelineStages(message) {
  const rows = []
  const rowMap = new Map()
  for (const event of traceActivityEvents(message)) {
    const key = agentEventRowKey(event)
    if (!key) continue
    let row = rowMap.get(key)
    if (!row) {
      row = createAgentRow(key, event, rows.length + 1)
      rowMap.set(key, row)
      rows.push(row)
    }
    mergeAgentRowEvent(row, event)
  }
  if (!rows.some((row) => row.id === 'answer') && messageText(message).trim()) {
    rows.push({
      id: 'answer',
      index: rows.length + 1,
      title: props.locale === 'zh' ? '生成回答' : 'Generate answer',
      subtitle: props.locale === 'zh' ? '基于证据生成回答' : 'Generate answer from evidence',
      status: 'completed',
      chips: [props.locale === 'zh' ? '已生成' : 'written'],
      details: [],
      latestEvent: null,
    })
  }
  return rows.map((row, index) => {
    const finalized = finalizeAgentRow(row, message)
    finalized.index = index + 1
    finalized.expanded = agentRowExpanded(message, finalized)
    return finalized
  })
}

function agentStageExpansionKey(message, stageId) {
  return `${message?.id || 'message'}:${stageId}`
}

function agentStageExpandedByUser(message, stageId) {
  return Boolean(expandedAgentStages.value[agentStageExpansionKey(message, stageId)])
}

function agentRowExpanded(message, row) {
  return row.status === 'running'
    || row.status === 'error'
    || agentStageExpandedByUser(message, row.id)
}

function toggleAgentStage(message, stage) {
  if (!stage?.details?.length) return
  const key = agentStageExpansionKey(message, stage.id)
  expandedAgentStages.value = {
    ...expandedAgentStages.value,
    [key]: !expandedAgentStages.value[key],
  }
}

function agentPanelExpanded(message) {
  if (message.status === 'running' || messageHasRetrievalIssue(message)) return true
  return Boolean(expandedAgentPanels.value[message?.id || 'message'])
}

function toggleAgentPanel(message) {
  if (message.status === 'running') return
  const key = message?.id || 'message'
  expandedAgentPanels.value = {
    ...expandedAgentPanels.value,
    [key]: !expandedAgentPanels.value[key],
  }
}

function agentTimelineVisible(message) {
  return agentProcessVisible(message)
}

function traceGateStatus(message) {
  return String(message.retrievalTrace?.finalizeGate?.status || '')
}

function messageHasRetrievalIssue(message) {
  return message.status === 'failed' || traceGateStatus(message) === 'insufficient'
}

function agentProcessVisible(message) {
  return message.role === 'assistant'
    && (message.status === 'running' || message.retrievalTrace || messageHasRetrievalIssue(message))
    && traceActivityEvents(message).length > 0
}

function eventType(event) {
  return String(event?.type || event?.eventType || event?.step || '')
}

function eventTitle(event) {
  const type = eventType(event)
  const tool = eventToolName(event)
  if (['tool_call', 'tool_result'].includes(type) && tool) {
    return formatTraceStep(tool)
  }
  if (type === 'start') return formatTraceStep('start')
  if (type === 'retrieval_round') {
    return event?.status === 'running'
      ? (props.locale === 'zh' ? '收集初始证据' : 'Gather initial evidence')
      : (props.locale === 'zh' ? '初始证据完成' : 'Initial evidence ready')
  }
  if (['judge_start', 'judge_result', 'judge_stop'].includes(type) || event?.step === 'finalize_answer') {
    const title = String(event?.title || '')
    if (title.includes('M4')) return props.locale === 'zh' ? 'LLM 证据判断' : 'LLM evidence check'
    if (title.includes('M3')) return props.locale === 'zh' ? '规则证据判断' : 'Rule evidence check'
    return formatTraceStep('finalize_answer')
  }
  if (type === 'answer_start' || event?.step === 'generate_answer') return formatTraceStep('generate_answer')
  return String(event?.title || formatTraceStep(event?.step || type))
}

function eventSummary(event) {
  const type = eventType(event)
  const count = toolResultCount(event)
  if (type === 'start') {
    return props.locale === 'zh'
      ? '准备本地检索和证据判断'
      : 'Preparing local retrieval and evidence checks'
  }
  if (type === 'retrieval_round') {
    if (event?.status === 'running') {
      return props.locale === 'zh'
        ? '正在阅读结构、章节、正文和页面概览'
        : 'Reading structure, sections, text, and page overview'
    }
    return props.locale === 'zh'
      ? '已完成初始证据收集'
      : 'Initial evidence collection finished'
  }
  if (type === 'tool_call') {
    return props.locale === 'zh'
      ? `正在${eventTitle(event)}`
      : `Running ${eventTitle(event).toLowerCase()}`
  }
  if (type === 'tool_result' && count !== null) {
    return props.locale === 'zh'
      ? `找到 ${count} 个候选结果`
      : `Found ${count} candidate results`
  }
  if (type === 'answer_start') {
    return props.locale === 'zh'
      ? '正在基于证据生成回答'
      : 'Generating an answer from the evidence'
  }
  const judgeStatus = event?.judge?.status
  if (['judge_start', 'judge_result', 'judge_stop'].includes(type)) {
    if (type === 'judge_start') {
      return props.locale === 'zh'
        ? '正在判断证据是否足够'
        : 'Checking whether the evidence is sufficient'
    }
    if (judgeStatus) {
      return props.locale === 'zh'
        ? `证据判断：${formatJudgeStatus(judgeStatus)}`
        : `Evidence check: ${formatJudgeStatus(judgeStatus)}`
    }
  }
  return String(event?.summary || event?.detail || '').trim()
}

function agentProcessStats(message) {
  // Count the rows the user actually sees (one per logical stage), not the raw
  // backend event stream — a single retrieval/judge stage can emit many events
  // (tool_call + tool_result + judge_start/result/stop per round), which made
  // the badge show inflated numbers like "44 steps" for a 7-row timeline.
  const rows = agentTimelineStages(message)
  const completed = rows.filter((row) => row.status && row.status !== 'running').length
  return {
    steps: rows.length,
    completed,
  }
}

function agentProcessState(message) {
  const latest = latestActivityEvent(message)
  if (message.status === 'running') {
    const type = eventType(latest)
    if (type === 'answer_start' || latest?.step === 'generate_answer') {
      return props.locale === 'zh' ? '正在生成回答' : 'Generating answer'
    }
    if (['judge_start', 'judge_result', 'judge_stop'].includes(type) || latest?.step === 'finalize_answer') {
      return props.locale === 'zh' ? '正在判断证据' : 'Checking evidence'
    }
    return props.locale === 'zh' ? '正在查找证据' : 'Finding evidence'
  }
  if (message.status === 'failed') return props.locale === 'zh' ? '已失败' : 'Failed'
  if (traceGateStatus(message) === 'insufficient') {
    return props.locale === 'zh' ? '证据不足' : 'Insufficient evidence'
  }
  return props.locale === 'zh' ? '证据检查完成' : 'Evidence check complete'
}

function agentProcessSubline(message) {
  const latest = latestActivityEvent(message)
  const stats = agentProcessStats(message)
  if (message.status === 'running') {
    const current = eventTitle(latest)
    return props.locale === 'zh'
      ? `已完成 ${stats.completed} 步 · 当前：${current}`
      : `${stats.completed} steps complete · Current: ${current}`
  }
  if (message.status === 'failed') return eventSummary(latest) || agentActivitySummary(message)
  if (traceGateStatus(message) === 'insufficient') {
    return props.locale === 'zh'
      ? '没有找到足够可靠的证据，详情里保留了检索过程'
      : 'Not enough reliable evidence was found; retrieval details are preserved'
  }
  return props.locale === 'zh'
    ? `完成 ${stats.steps} 步证据检查`
    : `Completed ${stats.steps} evidence-check steps`
}

function agentProcessBadge(message) {
  const stats = agentProcessStats(message)
  if (message.status === 'running') {
    return props.locale === 'zh'
      ? `${stats.completed}/${stats.steps} 步`
      : `${stats.completed}/${stats.steps} steps`
  }
  return props.locale === 'zh'
    ? `${stats.steps} 步`
    : `${stats.steps} steps`
}

function agentActivityDetailsLabel() {
  return props.locale === 'zh' ? '调试 Trace' : 'Debug trace'
}

function eventToolName(event) {
  return String(event?.tool?.name || event?.step || '')
}

function formatJudgeStatus(status) {
  const normalized = String(status || '')
  if (props.locale !== 'zh') return normalized || 'unknown'
  const labels = {
    answerable: '证据足够',
    needs_more_evidence: '需要更多证据',
    insufficient: '证据不足',
    skipped: '已跳过',
    unknown: '未知',
  }
  return labels[normalized] || normalized || labels.unknown
}

function toolResultCount(event) {
  const count = event?.result?.count
  return Number.isFinite(Number(count)) ? Number(count) : null
}

function traceCandidatePreview(message) {
  return (message.retrievalTrace?.candidates || []).slice(0, 6)
}

function traceJudgeDetails(message) {
  const gate = message.retrievalTrace?.finalizeGate
  if (!gate || typeof gate !== 'object') return null
  const runtime = String(gate.runtime || '')
  const reason = String(gate.reason || '')
  const missing = Array.isArray(gate.missing)
    ? gate.missing.map((item) => String(item)).filter(Boolean)
    : []
  const nextTool = gate.nextToolCall && typeof gate.nextToolCall === 'object'
    ? gate.nextToolCall
    : null
  if (!runtime && !reason && !missing.length && !nextTool) return null
  return {
    runtime,
    reason,
    missing,
    nextTool,
  }
}

function traceJudgeTitle() {
  return props.locale === 'zh' ? '判定' : 'Judge'
}

function traceJudgeLabel(key) {
  const zh = {
    runtime: '运行',
    reason: '原因',
    missing: '缺少',
    nextTool: '下个工具',
  }
  const en = {
    runtime: 'Runtime',
    reason: 'Reason',
    missing: 'Missing',
    nextTool: 'Next tool',
  }
  return props.locale === 'zh' ? zh[key] : en[key]
}

function formatNextToolCall(toolCall) {
  if (!toolCall) return ''
  const tool = String(toolCall.tool || '')
  const args = toolCall.args && typeof toolCall.args === 'object'
    ? Object.entries(toolCall.args)
      .map(([key, value]) => `${key}=${typeof value === 'string' ? value : JSON.stringify(value)}`)
      .join(' ')
    : ''
  return [tool, args].filter(Boolean).join(' · ')
}

function fallbackEvidenceChain(message) {
  return (message.citations || []).map((citation) => ({
    citationId: citation.id,
    label: citation.label,
    page: citation.page,
    blockId: citation.blockId,
    sectionTitle: citation.sectionTitle || null,
    source: citation.source,
    quote: citation.quote,
  }))
}

function evidenceItems(message) {
  const items = message.retrievalTrace?.evidenceChain
  return items?.length ? items : fallbackEvidenceChain(message)
}

function evidencePreviewItems(message) {
  return evidenceItems(message).slice(0, 4)
}

function resolveCitation(message, evidence) {
  return (message.citations || []).find((citation) => citation.id === evidence.citationId) || null
}

function evidenceSourceLabel(source) {
  const labels = {
    selection: props.ui.selectedText,
    open_section: 'Section',
    open_pages: 'Page',
    inspect_tables: 'Table',
    open_table: 'Table',
    table_fact: 'Table',
    visual_asset: 'Visual',
    open_visual: 'Visual',
    analyze_visual: 'Visual',
    fts: 'FTS',
    'client-context': 'Context',
  }
  const zhLabels = {
    selection: props.ui.selectedText,
    open_section: '章节',
    open_pages: '页面',
    inspect_tables: '表格',
    open_table: '表格',
    table_fact: '表格',
    visual_asset: '视觉证据',
    open_visual: '视觉证据',
    analyze_visual: '视觉证据',
    fts: '文本检索',
    'client-context': '上下文',
  }
  return props.locale === 'zh'
    ? (zhLabels[source] || source)
    : (labels[source] || source)
}

</script>

<template>
  <aside class="chat-shell" :style="{ width: collapsed ? '44px' : `${width}px` }" :class="{ collapsed }">
    <button class="collapse-btn" type="button" @click="emit('toggle-collapse')">
      {{ collapsed ? '❮' : '❯' }}
    </button>

    <button
      v-if="collapsed"
      type="button"
      class="collapsed-rail"
      :aria-label="ui.expand"
      @click="emit('toggle-collapse')"
    >
      <span>{{ ui.chat }}</span>
    </button>

    <template v-else>
      <div class="chat-header" data-tauri-drag-region @mousedown="startWindowDrag">
        <div>
          <div class="chat-title">{{ ui.chat }}</div>
          <div class="chat-subtitle">{{ ui.currentDoc }}: {{ document.shortTitle }}</div>
        </div>
        <div class="chat-header-actions">
          <div class="chat-tabs">
            <span class="active">{{ ui.chat }}</span>
            <span>{{ ui.notes }}</span>
          </div>
          <button
            type="button"
            class="chat-clear-btn"
            :disabled="!hasChatHistory"
            :title="ui.clearChatHistory"
            :aria-label="ui.clearChatHistory"
            @click="emit('clear-history')"
          >
            {{ ui.clearChatHistoryShort }}
          </button>
        </div>
      </div>

      <div
        ref="messageListRef"
        class="message-list"
        :class="{ 'is-scrolling': messageListScrolling }"
        @scroll="updateMessageScrollState"
        @wheel.passive="markUserScrolledMessages"
        @touchstart.passive="markUserScrolledMessages"
        @pointerdown="markUserScrolledMessages"
        @keydown="markUserScrolledMessages"
      >
        <div v-if="!document.chatReady" class="prepare-card">
          <div class="prepare-title">{{ ui.preparingDocument }}</div>
          <div class="prepare-line">- {{ ui.extractingBlocks }}</div>
          <div class="prepare-line">- {{ ui.mappingCitations }}</div>
          <div class="prepare-line">- {{ ui.preparingContext }}</div>
        </div>

        <template v-else>
          <div v-if="!visibleMessages.length" class="chat-empty-state">
            <div class="chat-empty-title">{{ ui.chatEmptyTitle }}</div>
            <div class="chat-empty-copy">{{ ui.chatEmptyHint }}</div>
          </div>
          <article
            v-for="message in visibleMessages"
            :key="message.id"
            class="message-card"
            :class="[message.role, message.status]"
          >
            <div class="message-role">{{ message.role === 'assistant' ? ui.assistant : ui.user }}</div>
            <img
              v-if="message.imageDataUrl"
              :src="message.imageDataUrl"
              :alt="ui.attach"
              class="message-image"
            />
            <section
              v-if="agentTimelineVisible(message)"
              class="agent-process"
              :class="{
                running: message.status === 'running',
                done: message.status !== 'running',
                collapsed: !agentPanelExpanded(message),
              }"
            >
              <button
                type="button"
                class="agent-process-head"
                :disabled="message.status === 'running'"
                :aria-expanded="agentPanelExpanded(message)"
                @click="toggleAgentPanel(message)"
              >
                <div class="agent-process-leading">
                  <span class="agent-process-pulse" aria-hidden="true"></span>
                  <div>
                    <div class="agent-process-kicker">{{ ui.agentActivity }}</div>
                    <div class="agent-process-title">{{ agentProcessState(message) }}</div>
                  </div>
                </div>
                <span class="agent-process-actions">
                  <span class="agent-process-badge">{{ agentProcessBadge(message) }}</span>
                  <span
                    v-if="message.status !== 'running'"
                    class="agent-process-toggle"
                    :class="{ open: agentPanelExpanded(message) }"
                    aria-hidden="true"
                  >
                    ›
                  </span>
                </span>
              </button>
              <div class="agent-process-subline">{{ agentProcessSubline(message) }}</div>
              <div v-if="agentPanelExpanded(message)" class="agent-timeline" role="list">
                <div
                  v-for="stage in agentTimelineStages(message)"
                  :key="`${message.id}-agent-stage-${stage.id}`"
                  class="agent-stage"
                  :class="[stage.status, { expanded: stage.expanded }]"
                  role="listitem"
                >
                  <div class="agent-stage-rail" aria-hidden="true">
                    <span class="agent-stage-dot"></span>
                  </div>
                  <div class="agent-stage-card">
                    <div class="agent-stage-row">
                      <div class="agent-stage-title-wrap">
                        <span class="agent-stage-index">{{ stage.index }}.</span>
                        <span class="agent-stage-title">{{ stage.title }}</span>
                        <span class="agent-stage-subtitle">{{ stage.subtitle }}</span>
                      </div>
                      <div class="agent-stage-meta">
                        <span
                          v-for="chip in stage.chips"
                          :key="`${message.id}-${stage.id}-${chip}`"
                          class="agent-stage-chip"
                        >
                          {{ chip }}
                        </span>
                        <span class="agent-stage-status">{{ formatTraceStatus(stage.status) }}</span>
                        <button
                          v-if="stage.details.length"
                          type="button"
                          class="agent-stage-toggle"
                          :class="{ open: stage.expanded }"
                          :aria-label="stage.expanded
                            ? (locale === 'zh' ? '收起' : 'Collapse')
                            : (locale === 'zh' ? '展开' : 'Expand')"
                          :aria-expanded="stage.expanded"
                          @click="toggleAgentStage(message, stage)"
                        >
                          <span aria-hidden="true">›</span>
                        </button>
                      </div>
                    </div>
                    <div v-if="stage.expanded && stage.details.length" class="agent-stage-detail">
                      <div
                        v-for="detail in stage.details"
                        :key="`${message.id}-${stage.id}-${detail}`"
                        class="agent-stage-detail-line"
                      >
                        {{ detail }}
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </section>
            <details
              v-if="reasoningText(message)"
              class="reasoning-panel"
              :open="message.status === 'running'"
            >
              <summary>
                <span>{{ ui.thinking }}</span>
                <span>{{ message.status === 'running' ? ui.thinkingStreaming : ui.thinkingComplete }}</span>
              </summary>
              <div class="reasoning-content">{{ reasoningText(message) }}</div>
            </details>
            <MarkdownText
              v-if="messageText(message)"
              class="message-content"
              :text="messageText(message)"
              :loading="message.role === 'assistant' && message.status === 'running'"
            />
            <div v-else-if="isWaitingForAnswer(message) && !agentProcessVisible(message)" class="message-loading" :aria-label="runningStatusLabel(message)">
              <span class="loading-dots" aria-hidden="true">
                <span></span>
                <span></span>
                <span></span>
              </span>
              <span>{{ runningStatusLabel(message) }}</span>
            </div>
            <div v-if="message.provider" class="message-provider">{{ message.provider }}</div>

            <div v-if="evidenceItems(message).length" class="evidence-group">
              <div class="evidence-strip">
                <span class="evidence-strip-label">{{ ui.evidence }}</span>
                <button
                  v-for="evidence in evidencePreviewItems(message)"
                  :key="`${message.id}-preview-${evidence.citationId}`"
                  class="evidence-chip"
                  :class="{ active: evidence.citationId === activeCitationId }"
                  :title="evidence.quote"
                  @click="resolveCitation(message, evidence) && emit('citation-click', resolveCitation(message, evidence))"
                >
                  <span>{{ evidence.label }}</span>
                  <span>{{ locale === 'zh' ? `${ui.page}${evidence.page}` : `p${evidence.page}` }}</span>
                  <span>{{ evidence.sectionTitle || evidenceSourceLabel(evidence.source) }}</span>
                </button>
                <span v-if="evidenceItems(message).length > evidencePreviewItems(message).length" class="evidence-more">
                  +{{ evidenceItems(message).length - evidencePreviewItems(message).length }}
                </span>
              </div>
            </div>

            <details
              v-if="message.retrievalTrace || message.activityEvents?.length"
              class="agent-activity agent-trace-drawer"
            >
              <summary>
                <span>{{ agentActivityDetailsLabel() }}</span>
                <span class="agent-activity-summary">
                  {{ agentActivitySummary(message) }}
                </span>
              </summary>
              <div v-if="message.retrievalTrace" class="agent-activity-meta">
                <span>{{ ui.traceIntent }} {{ message.retrievalTrace?.intent }}</span>
                <span>{{ ui.traceRun }} {{ message.retrievalTrace?.runId }}</span>
              </div>
              <div v-if="traceJudgeDetails(message)" class="trace-section trace-judge">
                <div class="trace-section-title">{{ traceJudgeTitle() }}</div>
                <div class="trace-judge-grid">
                  <div v-if="traceJudgeDetails(message).runtime" class="trace-judge-row">
                    <span>{{ traceJudgeLabel('runtime') }}</span>
                    <span>{{ traceJudgeDetails(message).runtime }}</span>
                  </div>
                  <div v-if="traceJudgeDetails(message).reason" class="trace-judge-row">
                    <span>{{ traceJudgeLabel('reason') }}</span>
                    <span>{{ traceJudgeDetails(message).reason }}</span>
                  </div>
                  <div v-if="traceJudgeDetails(message).missing.length" class="trace-judge-row">
                    <span>{{ traceJudgeLabel('missing') }}</span>
                    <span>{{ traceJudgeDetails(message).missing.join(', ') }}</span>
                  </div>
                  <div v-if="traceJudgeDetails(message).nextTool" class="trace-judge-row">
                    <span>{{ traceJudgeLabel('nextTool') }}</span>
                    <span>{{ formatNextToolCall(traceJudgeDetails(message).nextTool) }}</span>
                  </div>
                </div>
              </div>
              <div v-if="traceActivityEvents(message).length" class="agent-step-list">
                <div
                  v-for="(event, index) in traceActivityEvents(message)"
                  :key="`${message.id}-trace-event-${index}`"
                  class="agent-step"
                  :class="event.status"
                >
                  <div class="agent-step-marker"></div>
                  <div class="agent-step-body">
                    <div class="agent-step-head">
                      <span>{{ eventTitle(event) }}</span>
                      <span>{{ formatTraceStatus(event.status) }}</span>
                    </div>
                    <div class="agent-step-detail">{{ eventSummary(event) }}</div>
                    <div v-if="toolResultCount(event) !== null" class="agent-step-detail muted">
                      {{ locale === 'zh' ? '工具结果数' : 'Tool results' }}: {{ toolResultCount(event) }}
                    </div>
                  </div>
                </div>
              </div>
              <div v-if="message.retrievalTrace?.compact" class="trace-section">
                <div class="trace-section-title">{{ ui.traceCompact }}</div>
                <div class="trace-meta">
                  <span>{{ ui.traceCompactTrigger }} {{ message.retrievalTrace?.compact?.trigger }}</span>
                  <span>{{ ui.traceCompactRetained }} {{ message.retrievalTrace?.compact?.retainedRecentTurns }}</span>
                  <span>{{ ui.traceCompactSummarized }} {{ message.retrievalTrace?.compact?.summarizedTurns }}</span>
                </div>
              </div>
              <div v-if="message.retrievalTrace?.sessionSummary" class="trace-section">
                <div class="trace-section-title">{{ ui.traceSessionSummary }}</div>
                <div class="trace-summary-card">{{ message.retrievalTrace?.sessionSummary }}</div>
              </div>
              <div v-if="message.retrievalTrace?.treeNodes?.length" class="trace-section">
                <div class="trace-section-title">{{ ui.traceTreeNodes }}</div>
                <div class="trace-chip-list">
                  <span v-for="node in message.retrievalTrace?.treeNodes || []" :key="node.id" class="trace-chip">
                    {{ node.title }}
                  </span>
                </div>
              </div>
              <div v-if="message.retrievalTrace?.candidates?.length" class="trace-section">
                <div class="trace-section-title">{{ ui.traceCandidates }}</div>
                <div class="trace-candidate-list">
                  <div v-for="(candidate, index) in traceCandidatePreview(message)" :key="`${message.id}-${index}`" class="trace-candidate">
                    <div class="trace-candidate-head">
                      <span>{{ candidate.source }}</span>
                      <span>{{ locale === 'zh' ? `${ui.page}${candidate.page}` : `p${candidate.page}` }}</span>
                    </div>
                    <div class="trace-candidate-text">{{ candidate.quote }}</div>
                  </div>
                </div>
              </div>
            </details>
          </article>
        </template>
        <button
          v-if="showJumpToLatest"
          type="button"
          class="jump-to-latest"
          :title="ui.jumpToLatest"
          :aria-label="ui.jumpToLatest"
          @click="jumpToLatestMessage"
        >
          ↓
        </button>
      </div>

      <form
        class="chat-composer"
        @submit.prevent="handleSubmit"
        @paste="handleComposerPaste"
        @dragover.prevent
        @drop.prevent="handleComposerDrop"
      >
        <input
          ref="fileInputRef"
          class="composer-file-input"
          type="file"
          accept="image/*"
          @change="handleImagePicked"
        />
        <div v-if="supportsVision" class="composer-hint">
          {{ ui.imageDropHint }}
        </div>

        <div v-if="pendingSelection" class="pending-selection">
          <div class="pending-selection-head">
            <span>{{ pendingSelectionLabel }}</span>
            <button type="button" :title="ui.cancel" :aria-label="ui.cancel" @click="emit('clear-selection')">×</button>
          </div>
          <div class="pending-selection-text">{{ pendingSelectionPreview }}</div>
        </div>

        <div v-if="pendingImageDataUrl" class="pending-image">
          <img :src="pendingImageDataUrl" :alt="pendingImageName || ui.attach" class="pending-image-preview" />
          <div class="pending-image-meta">
            <div class="pending-image-name">{{ pendingImageName || ui.attach }}</div>
            <button type="button" class="pending-image-remove" @click="clearPendingImage">×</button>
          </div>
        </div>

        <textarea
          ref="composerTextareaRef"
          :disabled="!chatInputEnabled"
          :placeholder="
            !document.chatReady
              ? ui.inputDisabled
              : !modelConfigured
                ? ui.modelNotConfiguredHint
                : supportsVision
                  ? ui.imageInputPlaceholder
                  : ui.inputPlaceholder
          "
          @keydown="handleComposerKeydown"
        />

        <div class="composer-footer">
          <div class="composer-left">
            <button v-if="supportsVision" class="icon-btn" type="button" :disabled="!chatInputEnabled" :title="ui.attach" :aria-label="ui.attach" @click="openImagePicker">
              <span class="icon-plus" aria-hidden="true"></span>
            </button>
            <div v-if="document.chatReady" class="capability-pill" :title="capabilityTitle">
              <span class="capability-icon text" :title="ui.capabilityText"></span>
              <span v-if="supportsVision" class="capability-icon vision" :title="ui.capabilityVision"></span>
            </div>
          </div>

          <div class="composer-right">
            <label class="model-select-shell">
              <select
                :value="currentModelId"
                :disabled="!modelConfigured"
                @change="emit('update:model-id', $event.target.value)"
              >
                <option v-for="model in availableModels" :key="model.id" :value="model.id">
                  {{ modelOptionLabel(model) }}
                </option>
              </select>
            </label>

            <button class="submit-btn" :disabled="!chatInputEnabled" type="submit" :title="ui.sendMessage" :aria-label="ui.sendMessage">
              <span class="icon-send" aria-hidden="true"></span>
            </button>
          </div>
        </div>
      </form>
    </template>
  </aside>
</template>

<style scoped>
.chat-shell {
  position: relative;
  border-left: 1px solid var(--line-soft);
  background: var(--bg-panel);
  transition: width 0.18s ease;
  display: flex;
  flex-direction: column;
  min-width: 44px;
}

.collapse-btn {
  position: absolute;
  left: 0;
  top: 16px;
  transform: translateX(-50%);
  width: 24px;
  height: 34px;
  border-radius: 999px;
  border: 1px solid var(--line-soft);
  background: var(--bg-elevated);
  color: var(--text-secondary);
  cursor: pointer;
  z-index: 3;
}

.collapsed-rail {
  width: 100%;
  flex: 1;
  border: 0;
  background: transparent;
  display: flex;
  justify-content: center;
  align-items: center;
  color: var(--text-secondary);
  font-size: 12px;
  letter-spacing: 0.12em;
  writing-mode: vertical-rl;
  transform: rotate(180deg);
  cursor: pointer;
}

.collapsed-rail:hover {
  color: var(--text-primary);
  background: rgba(255, 255, 255, 0.035);
}

.chat-header,
.message-list,
.chat-composer {
  width: 100%;
}

.chat-header {
  padding: 18px 18px 14px;
  border-bottom: 1px solid var(--line-soft);
  display: flex;
  justify-content: space-between;
  gap: 12px;
}

.chat-title {
  color: var(--text-primary);
  font-size: 16px;
  font-weight: 700;
}

.chat-subtitle {
  margin-top: 4px;
  color: var(--text-muted);
  font-size: 12px;
}

.chat-tabs {
  display: flex;
  gap: 8px;
  align-items: center;
  color: var(--text-muted);
  font-size: 12px;
  text-transform: uppercase;
}

.chat-tabs .active {
  color: var(--text-primary);
}

.chat-header-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
}

.chat-clear-btn {
  border: 1px solid var(--line-soft);
  border-radius: 999px;
  padding: 5px 9px;
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1;
  cursor: pointer;
}

.chat-clear-btn:hover:not(:disabled) {
  border-color: rgba(255, 179, 179, 0.34);
  color: #ffd2d2;
}

.chat-clear-btn:disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

.message-list {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow: auto;
  scrollbar-width: thin;
  scrollbar-color: transparent transparent;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.message-list::-webkit-scrollbar {
  width: 8px;
  height: 8px;
}

.message-list::-webkit-scrollbar-track {
  background: transparent;
}

.message-list::-webkit-scrollbar-thumb {
  border: 2px solid transparent;
  border-radius: 999px;
  background: transparent;
  background-clip: padding-box;
}

.message-list:hover::-webkit-scrollbar-thumb,
.message-list.is-scrolling::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.2);
}

.message-list:hover,
.message-list.is-scrolling {
  scrollbar-color: rgba(255, 255, 255, 0.2) transparent;
}

.jump-to-latest {
  position: sticky;
  right: 0;
  bottom: 0;
  align-self: flex-end;
  width: 34px;
  height: 34px;
  margin-top: -46px;
  border: 1px solid rgba(106, 169, 255, 0.28);
  border-radius: 999px;
  background: rgba(31, 41, 55, 0.92);
  color: var(--text-primary);
  cursor: pointer;
  box-shadow: 0 10px 28px rgba(0, 0, 0, 0.28);
  z-index: 2;
}

.jump-to-latest:hover {
  background: rgba(43, 57, 76, 0.96);
}

.message-card,
.prepare-card {
  border: 1px solid var(--line-soft);
  border-radius: 16px;
  padding: 14px;
  background: rgba(255, 255, 255, 0.02);
}

.message-card.user {
  background: rgba(106, 169, 255, 0.08);
}

.message-card.running {
  border-color: rgba(106, 169, 255, 0.22);
}

.message-card.failed {
  border-color: rgba(255, 179, 179, 0.24);
}

.chat-empty-state {
  margin: auto 0;
  padding: 0 10px;
  color: var(--text-secondary);
}

.chat-empty-title {
  color: var(--text-primary);
  font-size: 15px;
  font-weight: 700;
}

.chat-empty-copy {
  max-width: 30rem;
  margin-top: 8px;
  font-size: 13px;
  line-height: 1.65;
}

.message-role,
.prepare-title,
.citation-group-title {
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 8px;
  text-transform: uppercase;
  letter-spacing: 0.08em;
}

.prepare-line,
.citation-quote {
  white-space: pre-wrap;
  line-height: 1.65;
  color: var(--text-primary);
  font-size: 13px;
}

.message-content {
  color: var(--text-primary);
  font-size: 13px;
}

.message-loading {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  min-height: 24px;
  color: var(--text-secondary);
  font-size: 13px;
}

.loading-dots {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.loading-dots span {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  background: rgba(106, 169, 255, 0.78);
  animation: loading-dot-pulse 1s ease-in-out infinite;
}

.loading-dots span:nth-child(2) {
  animation-delay: 0.14s;
}

.loading-dots span:nth-child(3) {
  animation-delay: 0.28s;
}

@keyframes loading-dot-pulse {
  0%,
  80%,
  100% {
    opacity: 0.35;
    transform: translateY(0);
  }

  40% {
    opacity: 1;
    transform: translateY(-3px);
  }
}

.message-image {
  width: min(100%, 280px);
  max-height: 220px;
  object-fit: contain;
  border-radius: 12px;
  border: 1px solid var(--line-soft);
  margin-bottom: 10px;
  background: rgba(255, 255, 255, 0.04);
}

.prepare-line + .prepare-line {
  margin-top: 6px;
}

.message-provider {
  margin-top: 10px;
  color: var(--text-muted);
  font-size: 11px;
}

.citation-group {
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
}

.citation-chip {
  display: inline-flex;
  gap: 8px;
  margin-right: 8px;
  margin-bottom: 8px;
  padding: 7px 10px;
  border-radius: 999px;
  border: 1px solid var(--line-soft);
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-secondary);
  cursor: pointer;
}

.citation-chip.active {
  border-color: rgba(250, 204, 21, 0.45);
  background: rgba(250, 204, 21, 0.12);
  color: var(--text-primary);
}

.evidence-group {
  margin-top: 12px;
}

.evidence-strip {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}

.evidence-strip-label {
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  margin-right: 2px;
}

.evidence-chip {
  display: inline-flex;
  align-items: center;
  max-width: 100%;
  min-height: 28px;
  gap: 6px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.035);
  color: var(--text-secondary);
  cursor: pointer;
  padding: 0 9px;
  font-size: 11px;
}

.evidence-chip span:last-child {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 150px;
}

.evidence-chip:hover,
.evidence-chip.active {
  border-color: rgba(245, 180, 24, 0.4);
  background: rgba(245, 180, 24, 0.12);
  color: var(--text-primary);
}

.evidence-more {
  color: var(--text-muted);
  font-size: 11px;
}

.agent-process {
  margin: 0 0 12px;
  padding: 13px 13px 12px;
  border: 1px solid rgba(106, 169, 255, 0.18);
  border-radius: 8px;
  background:
    linear-gradient(180deg, rgba(106, 169, 255, 0.07), rgba(106, 169, 255, 0.025)),
    rgba(255, 255, 255, 0.018);
}

.agent-process.running {
  border-color: rgba(106, 169, 255, 0.32);
  box-shadow: inset 0 0 0 1px rgba(106, 169, 255, 0.04);
}

.agent-process.done {
  border-color: rgba(74, 222, 128, 0.18);
  background:
    linear-gradient(180deg, rgba(74, 222, 128, 0.045), rgba(106, 169, 255, 0.018)),
    rgba(255, 255, 255, 0.012);
}

.agent-process-head {
  width: 100%;
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: center;
  border: 0;
  background: transparent;
  padding: 0;
  color: inherit;
  text-align: left;
}

button.agent-process-head {
  cursor: pointer;
}

button.agent-process-head:disabled {
  cursor: default;
}

.agent-process-leading {
  display: flex;
  align-items: center;
  gap: 9px;
  min-width: 0;
}

.agent-process-pulse {
  width: 11px;
  height: 11px;
  flex: 0 0 auto;
  border-radius: 999px;
  background: rgba(106, 169, 255, 0.9);
  box-shadow: 0 0 0 4px rgba(106, 169, 255, 0.12), inset 0 0 0 2px rgba(10, 14, 22, 0.62);
}

.agent-process.done .agent-process-pulse {
  background: rgba(34, 197, 94, 0.92);
  box-shadow: 0 0 0 4px rgba(34, 197, 94, 0.12), inset 0 0 0 2px rgba(10, 14, 22, 0.62);
}

.agent-process.running .agent-process-pulse {
  animation: process-dot-pulse 1s ease-in-out infinite;
}

.agent-process-kicker {
  color: var(--text-muted);
  font-size: 10px;
  font-weight: 800;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.agent-process-title {
  margin-top: 4px;
  color: var(--text-primary);
  font-size: 14px;
  font-weight: 750;
}

.agent-process-badge {
  flex: 0 0 auto;
  min-width: 52px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.055);
  color: var(--text-secondary);
  font-size: 11px;
  font-weight: 750;
  white-space: nowrap;
}

.agent-process-actions {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: 0 0 auto;
}

.agent-process-toggle {
  width: 20px;
  height: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
  font-size: 16px;
  line-height: 1;
  transform: rotate(0deg);
  transition: transform 0.14s ease;
}

.agent-process-toggle.open {
  transform: rotate(90deg);
}

.agent-process-subline {
  margin-top: 8px;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.45;
}

.agent-process.collapsed {
  padding-bottom: 13px;
}

.agent-timeline {
  position: relative;
  display: grid;
  gap: 0;
  margin-top: 14px;
  overflow: hidden;
  border: 1px solid rgba(255, 255, 255, 0.07);
  border-radius: 8px;
  background: rgba(10, 14, 22, 0.16);
}

.agent-stage {
  display: grid;
  grid-template-columns: 28px minmax(0, 1fr);
  min-width: 0;
}

.agent-stage + .agent-stage .agent-stage-card {
  border-top: 1px solid rgba(255, 255, 255, 0.06);
}

.agent-stage-rail {
  position: relative;
  display: flex;
  justify-content: center;
  padding-top: 15px;
}

.agent-stage-rail::before {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  width: 1px;
  background: rgba(148, 163, 184, 0.24);
}

.agent-stage:first-child .agent-stage-rail::before {
  top: 16px;
}

.agent-stage:last-child .agent-stage-rail::before {
  bottom: calc(100% - 16px);
}

.agent-stage-dot {
  position: relative;
  z-index: 1;
  width: 12px;
  height: 12px;
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.52);
  background: var(--bg-panel);
}

.agent-stage.completed .agent-stage-dot {
  border-color: rgba(34, 197, 94, 0.84);
  background: rgba(34, 197, 94, 0.18);
  box-shadow: inset 0 0 0 3px var(--bg-panel);
}

.agent-stage.running .agent-stage-dot {
  border-color: rgba(106, 169, 255, 0.92);
  background: rgba(106, 169, 255, 0.3);
  box-shadow: 0 0 0 4px rgba(106, 169, 255, 0.12), inset 0 0 0 3px var(--bg-panel);
}

.agent-stage.error .agent-stage-dot {
  border-color: rgba(248, 113, 113, 0.86);
  background: rgba(248, 113, 113, 0.2);
  box-shadow: inset 0 0 0 3px var(--bg-panel);
}

.agent-stage.pending {
  opacity: 0.55;
}

.agent-stage-card {
  min-width: 0;
  padding: 11px 10px 11px 0;
}

.agent-stage.error .agent-stage-card {
  box-shadow: inset 2px 0 0 rgba(248, 113, 113, 0.62);
}

.agent-stage-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 10px;
  min-width: 0;
}

.agent-stage-title-wrap {
  display: flex;
  align-items: center;
  flex: 1 1 190px;
  min-width: 0;
  gap: 7px;
}

.agent-stage-index,
.agent-stage-title {
  color: var(--text-primary);
  font-size: 12px;
  font-weight: 760;
  white-space: nowrap;
}

.agent-stage-subtitle {
  min-width: 0;
  overflow: hidden;
  color: var(--text-muted);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.agent-stage-meta {
  display: inline-flex;
  align-items: center;
  justify-content: flex-end;
  flex-wrap: wrap;
  gap: 6px;
  flex: 0 1 auto;
  max-width: 100%;
  min-width: 0;
}

.agent-stage-toggle {
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  flex: 0 0 auto;
}

.agent-stage-toggle:hover {
  background: rgba(255, 255, 255, 0.055);
  color: var(--text-primary);
}

.agent-stage-toggle span {
  display: inline-block;
  font-size: 16px;
  line-height: 1;
  transform: rotate(0deg);
  transition: transform 0.14s ease;
}

.agent-stage-toggle.open span {
  transform: rotate(90deg);
}

.agent-stage-chip,
.agent-stage-status {
  display: inline-flex;
  align-items: center;
  min-height: 22px;
  max-width: 150px;
  overflow: hidden;
  padding: 0 8px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.045);
  color: var(--text-secondary);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.agent-stage-status {
  color: var(--text-muted);
}

.agent-stage.completed .agent-stage-status {
  color: rgba(74, 222, 128, 0.9);
}

.agent-stage.running .agent-stage-status {
  color: rgba(106, 169, 255, 0.96);
}

.agent-stage.error .agent-stage-status {
  color: rgba(248, 113, 113, 0.94);
}

.agent-stage-detail {
  display: grid;
  gap: 4px;
  margin-top: 8px;
  padding-left: 25px;
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.45;
}

.agent-stage-detail-line {
  position: relative;
  overflow-wrap: anywhere;
}

.agent-stage-detail-line::before {
  content: '';
  position: absolute;
  left: -13px;
  top: 0.72em;
  width: 4px;
  height: 4px;
  border-radius: 999px;
  background: rgba(106, 169, 255, 0.72);
}

@keyframes process-dot-pulse {
  0%,
  100% {
    opacity: 0.45;
    transform: scale(0.9);
  }

  50% {
    opacity: 1;
    transform: scale(1);
  }
}

.agent-activity {
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
}

.agent-activity summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  cursor: pointer;
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 700;
  list-style: none;
}

.agent-activity summary::-webkit-details-marker {
  display: none;
}

.agent-activity-summary {
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 500;
  text-align: right;
}

.agent-activity-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 12px;
  margin-top: 10px;
  color: var(--text-muted);
  font-size: 11px;
}

.agent-step-list {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 12px;
}

.agent-step {
  display: grid;
  grid-template-columns: 16px minmax(0, 1fr);
  gap: 8px;
  align-items: start;
}

.agent-step.skipped {
  opacity: 0.68;
}

.agent-step-marker {
  width: 8px;
  height: 8px;
  margin-top: 6px;
  border-radius: 999px;
  background: rgba(106, 169, 255, 0.8);
  box-shadow: 0 0 0 4px rgba(106, 169, 255, 0.08);
}

.agent-step.skipped .agent-step-marker {
  background: rgba(156, 163, 175, 0.7);
  box-shadow: 0 0 0 4px rgba(156, 163, 175, 0.06);
}

.agent-step-body {
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 12px;
  padding: 9px 11px;
  background: rgba(255, 255, 255, 0.025);
}

.agent-step-head {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 5px;
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 700;
}

.agent-step-head span:last-child {
  color: var(--text-muted);
  font-size: 10px;
  text-transform: uppercase;
  letter-spacing: 0.06em;
}

.agent-step-detail {
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
}

.agent-step-detail.muted {
  margin-top: 4px;
  font-size: 11px;
}

.reasoning-panel {
  margin: 0 0 10px;
}

.reasoning-panel summary {
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 0;
  color: var(--text-muted);
  font-size: 12px;
  font-weight: 600;
  list-style: none;
}

.reasoning-panel summary::-webkit-details-marker {
  display: none;
}

.reasoning-panel summary::before {
  content: '›';
  color: var(--text-muted);
  font-size: 14px;
  line-height: 1;
  transform: rotate(0deg);
  transition: transform 0.15s ease;
}

.reasoning-panel[open] summary::before {
  transform: rotate(90deg);
}

.reasoning-panel summary span:last-child {
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 500;
  text-transform: lowercase;
  letter-spacing: 0;
}

.reasoning-content {
  max-height: 220px;
  overflow: auto;
  margin: 8px 0 0 5px;
  padding: 0 0 0 12px;
  border-left: 1px solid rgba(148, 163, 184, 0.22);
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
}

.trace-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 12px;
  margin-top: 10px;
  color: var(--text-muted);
  font-size: 11px;
}

.trace-section {
  margin-top: 12px;
}

.trace-section-title {
  color: var(--text-secondary);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  margin-bottom: 8px;
}

.trace-judge-grid {
  display: grid;
  gap: 6px;
  color: var(--text-secondary);
  font-size: 12px;
}

.trace-judge-row {
  display: grid;
  grid-template-columns: 72px minmax(0, 1fr);
  gap: 10px;
  align-items: start;
}

.trace-judge-row span:first-child {
  color: var(--text-muted);
}

.trace-judge-row span:last-child {
  min-width: 0;
  overflow-wrap: anywhere;
}

.trace-chip-list {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.trace-chip {
  padding: 6px 10px;
  border-radius: 999px;
  border: 1px solid var(--line-soft);
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-secondary);
  font-size: 11px;
}

.trace-candidate-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.trace-candidate {
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 12px;
  padding: 10px 12px;
  background: rgba(255, 255, 255, 0.025);
}

.trace-candidate-head {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  color: var(--text-muted);
  font-size: 11px;
  margin-bottom: 6px;
}

.trace-candidate-text {
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.55;
  white-space: pre-wrap;
}

.trace-summary-card {
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 12px;
  padding: 10px 12px;
  background: rgba(255, 255, 255, 0.025);
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.55;
  white-space: pre-wrap;
}

.chat-composer {
  position: relative;
  padding: 18px;
  border-top: 1px solid var(--line-soft);
  display: flex;
  flex-direction: column;
  gap: 14px;
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.012), rgba(255, 255, 255, 0.035));
}

.composer-file-input {
  display: none;
}

.pending-selection {
  border: 1px solid rgba(250, 204, 21, 0.2);
  border-radius: 14px;
  background: rgba(250, 204, 21, 0.07);
  padding: 12px 14px;
}

.pending-selection-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  color: var(--text-primary);
  font-size: 12px;
  font-weight: 700;
}

.pending-selection-head button {
  width: 26px;
  height: 26px;
  border: none;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-secondary);
  cursor: pointer;
}

.pending-selection-text {
  margin-top: 8px;
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 1.55;
  max-height: 82px;
  overflow: auto;
}

.pending-image {
  display: flex;
  align-items: center;
  gap: 12px;
  border: 1px solid rgba(106, 169, 255, 0.22);
  border-radius: 14px;
  background: rgba(106, 169, 255, 0.08);
  padding: 10px 12px;
}

.pending-image-preview {
  width: 64px;
  height: 64px;
  flex: 0 0 auto;
  object-fit: cover;
  border-radius: 10px;
  border: 1px solid rgba(255, 255, 255, 0.12);
}

.pending-image-meta {
  min-width: 0;
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.pending-image-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-secondary);
  font-size: 12px;
}

.pending-image-remove {
  width: 26px;
  height: 26px;
  flex: 0 0 auto;
  border: none;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-secondary);
  cursor: pointer;
}

.chat-composer textarea {
  width: 100%;
  min-height: 122px;
  resize: none;
  border-radius: 22px;
  border: 1px solid var(--line-soft);
  background: rgba(255, 255, 255, 0.05);
  color: var(--text-primary);
  padding: 18px;
  outline: none;
  line-height: 1.6;
  font: inherit;
  font-size: 14px;
}

.chat-composer textarea:disabled,
.chat-composer button:disabled,
.model-select-shell select:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.composer-hint {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
}

.composer-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-height: 38px;
}

.composer-left,
.composer-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.icon-btn,
.submit-btn {
  width: 36px;
  height: 36px;
  border-radius: 999px;
  border: 1px solid var(--line-soft);
  background: rgba(255, 255, 255, 0.05);
  color: var(--text-secondary);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 16px;
}

.submit-btn {
  background: var(--accent-soft);
  border-color: rgba(106, 169, 255, 0.24);
  color: var(--text-primary);
}

.capability-pill {
  height: 36px;
  padding: 0 10px;
  border-radius: 999px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-muted);
  display: inline-flex;
  align-items: center;
  font-size: 11px;
  white-space: nowrap;
}

.capability-pill {
  gap: 6px;
  color: var(--text-secondary);
  cursor: default;
}

.capability-icon {
  position: relative;
  width: 18px;
  height: 18px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.08);
  flex: 0 0 auto;
}

.capability-icon.text::before {
  content: 'T';
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  font-weight: 700;
  color: var(--text-secondary);
}

.capability-icon.vision::before,
.capability-icon.vision::after {
  content: '';
  position: absolute;
}

.capability-icon.vision::before {
  left: 3px;
  right: 3px;
  top: 4px;
  bottom: 4px;
  border-radius: 4px;
  border: 1px solid rgba(255, 255, 255, 0.16);
}

.capability-icon.vision::after {
  width: 6px;
  height: 6px;
  right: 4px;
  top: 4px;
  border-radius: 999px;
  background: rgba(106, 169, 255, 0.65);
}

.icon-plus,
.icon-send {
  position: relative;
  width: 14px;
  height: 14px;
  display: inline-block;
}

.icon-plus::before,
.icon-plus::after,
.icon-send::before,
.icon-send::after {
  content: '';
  position: absolute;
}

.icon-plus::before {
  left: 6px;
  top: 2px;
  bottom: 2px;
  width: 2px;
  border-radius: 999px;
  background: currentColor;
}

.icon-plus::after {
  top: 6px;
  left: 2px;
  right: 2px;
  height: 2px;
  border-radius: 999px;
  background: currentColor;
}

.icon-send::before {
  left: 2px;
  top: 2px;
  width: 0;
  height: 0;
  border-top: 5px solid transparent;
  border-bottom: 5px solid transparent;
  border-left: 10px solid currentColor;
}

.icon-send::after {
  left: 3px;
  top: 6px;
  width: 6px;
  height: 2px;
  border-radius: 999px;
  background: rgba(23, 24, 27, 0.7);
}

.model-select-shell {
  position: relative;
  display: inline-flex;
  flex: 0 1 auto;
}

.model-select-shell select {
  height: 36px;
  min-width: 172px;
  max-width: 220px;
  padding: 0 30px 0 13px;
  border-radius: 999px;
  border: 1px solid transparent;
  appearance: none;
  background:
    linear-gradient(45deg, transparent 50%, var(--text-secondary) 50%) calc(100% - 17px) 50% / 5px 5px no-repeat,
    linear-gradient(135deg, var(--text-secondary) 50%, transparent 50%) calc(100% - 12px) 50% / 5px 5px no-repeat,
    rgba(255, 255, 255, 0.045);
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 12px;
  font-weight: 650;
  outline: none;
  white-space: nowrap;
  text-overflow: ellipsis;
  transition: background 140ms ease, color 140ms ease;
}

.model-select-shell select:hover,
.model-select-shell select:focus-visible {
  background:
    linear-gradient(45deg, transparent 50%, var(--text-primary) 50%) calc(100% - 17px) 50% / 5px 5px no-repeat,
    linear-gradient(135deg, var(--text-primary) 50%, transparent 50%) calc(100% - 12px) 50% / 5px 5px no-repeat,
    rgba(255, 255, 255, 0.085);
  color: var(--text-primary);
}
</style>
