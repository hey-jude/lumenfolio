<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import MarkdownText from './MarkdownText.vue'
import { startWindowDrag } from '../windowDrag'
import { testAttrs } from '../testAttrs'

const props = defineProps({
  // The active agent session (conversation). Messages live here, not on the
  // document. May be null before the first session is created.
  session: {
    type: Object,
    default: null,
  },
  // Open session tabs: [{ id, title, active }].
  sessions: {
    type: Array,
    default: () => [],
  },
  // All sessions for the history dropdown: [{ id, title, focusTitle, turnCount, active }].
  historyItems: {
    type: Array,
    default: () => [],
  },
  historyOpen: {
    type: Boolean,
    default: false,
  },
  notesOpen: {
    type: Boolean,
    default: false,
  },
  // The session's focus document (retrieval target + readiness + header label).
  document: {
    type: Object,
    required: true,
  },
  // The document the user is currently viewing in the reader. When it differs
  // from the focus document, the header offers to retarget the session's focus.
  viewedDocId: {
    type: String,
    default: '',
  },
  viewedDocName: {
    type: String,
    default: '',
  },
  // When set, the user is browsing a non-reader surface (e.g. "Daily Trending");
  // the focus row shows this instead of a stale focus-document label.
  browsingLabel: {
    type: String,
    default: '',
  },
  // Activity event id of the in-flight generation in ANY session (from the parent),
  // so the stop button works even after navigating away from the running session.
  runningEventId: {
    type: String,
    default: '',
  },
  allDocuments: {
    type: Array,
    default: () => [],
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
  'set-tab',
  'new-session',
  'export-chat',
  'stop-generation',
  'select-session',
  'close-session',
  'delete-session',
  'toggle-history',
  'close-history',
  'toggle-notes',
  'set-focus-doc',
  'edit-resend',
])

// Inline edit of the last user question ("edit & re-ask"). Only one message is
// editable at a time; submitting replaces the last turn (handled by the parent).
const editingMessageId = ref('')
const editDraft = ref('')

// A plain mouse wheel only emits vertical deltas, which the horizontally-only
// session tab strip ignores by default. Translate vertical wheel into
// horizontal scroll so a mouse (not just a trackpad) can reach overflowed tabs.
function onSessionTabsWheel(event) {
  const el = event.currentTarget
  if (!el || el.scrollWidth <= el.clientWidth) return
  // Respect genuine horizontal intent (trackpad / shift+wheel); only convert
  // the vertical component when it dominates.
  if (Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return
  el.scrollLeft += event.deltaY
  event.preventDefault()
}

// True when the user is reading a different document than the session focuses on.
const focusDiffersFromView = computed(() => Boolean(
  props.viewedDocId
    && props.document?.id
    && props.viewedDocId !== props.document.id
    && props.viewedDocId !== 'empty',
))

// The composer stays usable while a doc is still indexing — asking before it's
// ready just gets a natural-language "still indexing" reply (handled in the parent)
// instead of a blocking loading card.
const chatInputEnabled = computed(() => props.modelConfigured)
const supportsVision = computed(() => props.currentModel?.capabilities?.includes('vision'))
// Messages come from the active session. The welcome placeholder is keyed by
// session id so it is filtered out of the visible list.
const sessionMessages = computed(() => (props.session?.messages || [])
  .filter((message) => message && typeof message === 'object'))
const welcomePrefix = computed(() => `welcome-${props.session?.id || ''}`)
const visibleMessages = computed(() => sessionMessages.value
  .filter((message) => !String(message.id || '').startsWith(welcomePrefix.value)))
const hasChatHistory = computed(() => sessionMessages.value
  .some((message) => !String(message.id || '').startsWith(welcomePrefix.value)))
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
// Message ids whose evidence chips are expanded to the full chain (vs the "+N"
// preview). The full chain is already in memory, so expanding is instant.
const expandedEvidence = ref(new Set())
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

// --- @-mention of other indexed papers ---------------------------------------
// The INPUT TEXT is the source of truth: selecting a paper inserts the plain text
// "@<name>⁠ " at the caret (no contenteditable, so IME stays intact), and the
// set of referenced docs is DERIVED from which "@<name>⁠" tokens are still
// present in the text. This lets the user phrase per-document requests inline
// (e.g. "对于 @A 总结方法, 对于 @B 对比实验"), giving the LLM full positional semantics.
//
// The trailing U+2060 (WORD JOINER) is an invisible token terminator. It makes
// each "@<label>⁠" a self-delimited unit so that:
//   - one label can't be a substring of another (@GLM⁠ vs @GLM-5⁠),
//   - labels containing spaces still match as a whole,
//   - editing adjacent text never partially matches.
// It is stripped from the text before sending, so the LLM only sees "@<name>".
//
// mentionRegistry maps doc id -> its UNIQUE display label for the current draft
// (duplicate display names are disambiguated when added). A doc counts as a live
// reference only while its full "@<label>⁠" token remains in the textarea, so
// deleting the text deletes the reference.
const MENTION_MIME = 'application/x-lumenfolio-doc-id'
const MAX_REFERENCE_DOCS = 4
const MENTION_TERMINATOR = '⁠'
const mentionRegistry = ref({})
const composerText = ref('')

// The full inline token for a label, including the invisible terminator.
function mentionToken(label) {
  return `@${label}${MENTION_TERMINATOR}`
}
const mentionPickerOpen = ref(false)
const mentionFilter = ref('')
const mentionActiveIndex = ref(0)
const mentionSearchRef = ref(null)
// Per-document composer drafts so switching tabs preserves an unsent message
// (textarea text + its mention registry). The textarea stays UNCONTROLLED (no
// v-model) to keep IME intact; we save/restore its value imperatively on doc
// switch. In-memory only — drafts are not persisted across app restart.
const composerDrafts = new Map()

// Doc ids currently referenced, derived from the "@<label>⁠" tokens still present
// in the composer text (order = registry order). This is the single source of
// truth for chips and for the send payload. Matching uses the FULL token (with
// terminator), so it is exact — no substring collisions between labels.
const mentionedDocIds = computed(() => {
  const text = composerText.value
  const ids = []
  for (const [id, label] of Object.entries(mentionRegistry.value)) {
    if (text.includes(mentionToken(label))) {
      ids.push(id)
    }
  }
  return ids
})

// Keep composerText in sync with the uncontrolled textarea on every input, so the
// derived mention state (chips, count) updates as the user types or deletes "@…".
function handleComposerInput(event) {
  composerText.value = event.target.value
}

function captureDraft(docId) {
  if (!docId) return
  const text = composerTextareaRef.value?.value || ''
  if (text.trim() || Object.keys(mentionRegistry.value).length) {
    composerDrafts.set(docId, { text, registry: { ...mentionRegistry.value } })
  } else {
    composerDrafts.delete(docId)
  }
}

function restoreDraft(docId) {
  const draft = composerDrafts.get(docId) || { text: '', registry: {} }
  mentionRegistry.value = { ...draft.registry }
  composerText.value = draft.text
  closeMentionPicker()
  nextTick(() => {
    if (composerTextareaRef.value) composerTextareaRef.value.value = draft.text
  })
}

function docDisplayName(doc) {
  return doc?.shortTitle || doc?.title || ''
}

// Candidates: other documents that are indexed/chat-ready, minus the current doc
// and ones already mentioned, filtered by the live picker query.
const mentionCandidates = computed(() => {
  const filter = mentionFilter.value.trim().toLowerCase()
  return (props.allDocuments || [])
    .filter((doc) => doc && doc.id && doc.id !== props.document.id)
    .filter((doc) => doc.chatReady)
    .filter((doc) => !mentionedDocIds.value.includes(doc.id))
    .filter((doc) => !filter || docDisplayName(doc).toLowerCase().includes(filter))
    .slice(0, 8)
})

const mentionedDocs = computed(() => mentionedDocIds.value
  .map((id) => (props.allDocuments || []).find((doc) => doc.id === id))
  .filter(Boolean))

const mentionLimitReached = computed(() => mentionedDocIds.value.length >= MAX_REFERENCE_DOCS)

// Filter-independent: is there ANY other chat-ready doc to mention? Gates whether
// the "@" key hijacks input to open the picker (single-doc workspaces type "@" normally).
const hasMentionableDocs = computed(() => (props.allDocuments || [])
  .some((doc) => doc && doc.id && doc.id !== props.document.id
    && doc.chatReady && !mentionedDocIds.value.includes(doc.id)))

// Allocate a display label that is UNIQUE among the current registry, so two docs
// with the same short name don't collide on one registry key / one inline token.
function uniqueMentionLabel(baseLabel) {
  const taken = new Set(Object.values(mentionRegistry.value))
  if (!taken.has(baseLabel)) return baseLabel
  let n = 2
  while (taken.has(`${baseLabel} (${n})`)) n += 1
  return `${baseLabel} (${n})`
}

// Insert "@<label>⁠ " (with invisible terminator) at the caret and register the
// id -> label mapping. The text is the source of truth, so this is what makes the
// doc a live reference.
function addMention(id) {
  if (!id || id === props.document.id) return
  if (mentionedDocIds.value.includes(id)) return
  if (id in mentionRegistry.value) return
  if (mentionLimitReached.value) return
  const doc = (props.allDocuments || []).find((item) => item.id === id)
  if (!doc) return
  const label = uniqueMentionLabel(docDisplayName(doc))
  const token = `${mentionToken(label)} `
  const textarea = composerTextareaRef.value
  const current = textarea?.value ?? composerText.value
  const end = textarea?.selectionEnd ?? current.length
  // The "@" the user typed to trigger the picker may have slipped into the textarea
  // (preventDefault doesn't reliably stop it under IME/composition). Absorb a
  // trailing "@<partial>" immediately before the caret so we never produce "@@name".
  let start = textarea?.selectionStart ?? current.length
  const before = current.slice(0, start)
  const triggerMatch = before.match(/@[^@\s]*$/)
  if (triggerMatch) {
    start -= triggerMatch[0].length
  }
  const next = current.slice(0, start) + token + current.slice(end)
  mentionRegistry.value = { ...mentionRegistry.value, [id]: label }
  composerText.value = next
  if (textarea) {
    textarea.value = next
    const caret = start + token.length
    nextTick(() => {
      textarea.focus()
      textarea.setSelectionRange(caret, caret)
    })
  }
  closeMentionPicker()
}

// Click-selecting an option: insert it; addMention already restores focus + caret.
function selectMention(id) {
  addMention(id)
}

// Chip "×": remove the doc's full "@<label>⁠" token from the text (text is truth).
function removeMention(id) {
  const label = mentionRegistry.value[id]
  if (label === undefined) return
  const token = mentionToken(label)
  let text = composerTextareaRef.value?.value ?? composerText.value
  // Remove the token plus an immediately-following space if present, else the
  // bare token. Exact-match on the full token (with terminator) means no other
  // label or normal text can be hit.
  text = text.split(`${token} `).join('').split(token).join('')
  composerText.value = text
  if (composerTextareaRef.value) composerTextareaRef.value.value = text
  const nextRegistry = { ...mentionRegistry.value }
  delete nextRegistry[id]
  mentionRegistry.value = nextRegistry
}

function clearMentions() {
  mentionRegistry.value = {}
  composerText.value = ''
}

function openMentionPicker() {
  if (!chatInputEnabled.value || mentionLimitReached.value) return
  mentionFilter.value = ''
  mentionActiveIndex.value = 0
  mentionPickerOpen.value = true
  // Move focus into the picker's search box so typed characters filter the list
  // instead of leaking into the (uncontrolled) textarea.
  nextTick(() => mentionSearchRef.value?.focus())
}

function closeMentionPicker({ refocusComposer = false } = {}) {
  mentionPickerOpen.value = false
  mentionFilter.value = ''
  mentionActiveIndex.value = 0
  if (refocusComposer) nextTick(() => composerTextareaRef.value?.focus())
}

function handleMentionPickerKeydown(event) {
  if (!mentionPickerOpen.value) return
  const count = mentionCandidates.value.length
  if (event.key === 'Escape') {
    event.preventDefault()
    closeMentionPicker({ refocusComposer: true })
  } else if (event.key === 'ArrowDown') {
    event.preventDefault()
    if (count) mentionActiveIndex.value = (mentionActiveIndex.value + 1) % count
  } else if (event.key === 'ArrowUp') {
    event.preventDefault()
    if (count) mentionActiveIndex.value = (mentionActiveIndex.value - 1 + count) % count
  } else if (event.key === 'Enter') {
    event.preventDefault()
    const doc = mentionCandidates.value[mentionActiveIndex.value]
    if (doc) {
      // addMention closes the picker; return focus to the composer to keep typing.
      addMention(doc.id)
      nextTick(() => composerTextareaRef.value?.focus())
    } else {
      // No candidate to pick (e.g. no match): dismiss the picker instead of
      // swallowing Enter, so the user isn't stuck.
      closeMentionPicker({ refocusComposer: true })
    }
  }
}

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
  // Sync derived state to the latest textarea value (still containing the invisible
  // terminators), so mentionedDocIds resolves correctly, THEN strip the terminators
  // from the outgoing text so the LLM only sees clean "@<name>".
  composerText.value = text
  const ids = [...mentionedDocIds.value]
  const cleanText = text.split(MENTION_TERMINATOR).join('')
  emit('send', {
    text: cleanText || props.ui.imageOnlyPrompt,
    imageDataUrl: pendingImageDataUrl.value || '',
    imageName: pendingImageName.value || '',
    mentionedDocIds: ids,
  })
  event.target.reset()
  clearPendingImage()
  clearMentions()
  closeMentionPicker()
  composerDrafts.delete(props.session?.id || '')
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
  // "@" is a pure trigger: open the picker and DON'T let the character land in the
  // textarea (mentions live entirely in chip state). Only hijack when there is
  // actually another doc to mention, so single-doc workspaces type "@" normally.
  if (event.key === '@' && !event.isComposing && chatInputEnabled.value
      && hasMentionableDocs.value && !mentionLimitReached.value) {
    event.preventDefault()
    openMentionPicker()
    return
  }
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
  // A document dragged from the sidebar carries our custom MIME — add it as a
  // reference. This is checked before the image path so the two don't conflict.
  const droppedDocId = event.dataTransfer?.getData(MENTION_MIME)
  if (droppedDocId) {
    if (chatInputEnabled.value) addMention(droppedDocId)
    return
  }
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

// The last user question — the only one that can be edited & re-asked.
const lastUserMessage = computed(() => {
  const msgs = visibleMessages.value
  for (let i = msgs.length - 1; i >= 0; i -= 1) {
    if (msgs[i].role === 'user') return msgs[i]
  }
  return null
})

// Editing is blocked while the turn is still streaming.
const lastTurnRunning = computed(() => {
  const msgs = visibleMessages.value
  const last = msgs[msgs.length - 1]
  return Boolean(last && last.role === 'assistant' && last.status === 'running')
})
// The activity event id of the in-flight generation, for the stop button.
const runningActivityEventId = computed(() => {
  const msgs = visibleMessages.value
  const last = msgs[msgs.length - 1]
  return last && last.role === 'assistant' && last.status === 'running'
    ? (last.activityEventId || '')
    : ''
})

function isEditableUserMessage(message) {
  return (
    message.role === 'user'
    && message.id === lastUserMessage.value?.id
    && !lastTurnRunning.value
  )
}

function startEdit(message) {
  editingMessageId.value = message.id
  editDraft.value = messageText(message)
}

function cancelEdit() {
  editingMessageId.value = ''
  editDraft.value = ''
}

function submitEdit(message) {
  const text = editDraft.value.trim()
  if (!text) return
  emit('edit-resend', { messageId: message.id, text })
  cancelEdit()
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
  const distance = element.scrollHeight - element.scrollTop - element.clientHeight
  const nearBottom = distance <= 64
  const pinnedToBottom = distance <= 4
  // Direction-aware stick-to-bottom: ANY real upward scroll detaches auto-follow
  // so streaming output never yanks the view back down — even a light wheel-up
  // while still inside the 64px "near bottom" band (the old `&& !atBottom` guard
  // swallowed those, so you had to scroll hard to escape). Content growth during
  // streaming keeps scrollTop unchanged (only scrollHeight grows), so it is never
  // mistaken for a user scrolling up.
  if (scrollTop < lastMessageScrollTop - 1) {
    autoFollowMessages.value = false
    userScrolledMessages.value = true
  } else if (pinnedToBottom) {
    // Re-attach only when TRULY at the bottom, not merely near it — otherwise a
    // momentum settle a few px above the bottom would re-pin against the user.
    autoFollowMessages.value = true
    userScrolledMessages.value = false
  }
  lastMessageScrollTop = scrollTop
  showJumpToLatest.value = !nearBottom && visibleMessages.value.length > 0
}

function markUserScrolledMessages(event) {
  // An upward wheel gesture detaches auto-follow IMMEDIATELY, even when pinned to
  // the bottom — otherwise a light scroll-up during streaming is undone by the
  // next chunk's stick-to-bottom before the scroll handler can react.
  if (event && event.type === 'wheel' && event.deltaY < 0) {
    autoFollowMessages.value = false
    userScrolledMessages.value = true
    return
  }
  // Other gestures (touch/pointer/key) while not pinned to the bottom detach too;
  // the scroll handler then refines the state using scroll position/direction.
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

watch(() => props.session?.id, (newId, oldId) => {
  // Save the draft of the session we're leaving, restore the one we're entering.
  if (oldId) captureDraft(oldId)
  restoreDraft(newId || '')
  autoFollowMessages.value = true
  userScrolledMessages.value = false
  showJumpToLatest.value = false
  scrollMessagesToBottom({ force: true, settle: true })
})

watch(() => props.session?.chatHistoryLoaded, (loaded) => {
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
  if (trace?.finalizeGate?.bestEffort === true) return props.ui.traceStatusBestEffort
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

// True when the backend answered best-effort (gate not "answerable", but enough
// evidence to answer with stated limits) — stamped as bestEffort on the gate.
function traceGateBestEffort(message) {
  return message.retrievalTrace?.finalizeGate?.bestEffort === true
}

function messageHasRetrievalIssue(message) {
  // A best-effort answer is not an issue: it produced a real answer from the
  // evidence even though the gate didn't formally reach "answerable".
  if (traceGateBestEffort(message)) return false
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
  // Best-effort: the gate wasn't "answerable" (e.g. judge timed out) but we still
  // answered from the gathered evidence — show that, not a failure.
  if (traceGateBestEffort(message)) {
    return props.locale === 'zh' ? '基于现有证据作答' : 'Answered with available evidence'
  }
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
  if (traceGateBestEffort(message)) {
    return props.locale === 'zh'
      ? '证据检查未完成，已基于已检索到的证据作答（详情见检索过程）'
      : 'The evidence check did not complete; answered from the evidence gathered (see retrieval details)'
  }
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

// Friendly label for finalizeGate.runtime — keeps the Debug trace coherent across
// all gates (the unified agent loop as well as the legacy M3 seed / M4 judge).
function formatRuntime(runtime) {
  const value = String(runtime || '')
  const zh = {
    'unified-loop': '统一智能体循环',
    'm4-llm-judge': 'LLM 证据判官',
    'm3-rule-guard': '规则判官',
    'm3-heuristic-judge': '启发式判官',
  }
  const en = {
    'unified-loop': 'Unified agent loop',
    'm4-llm-judge': 'LLM judge',
    'm3-rule-guard': 'Rule guard',
    'm3-heuristic-judge': 'Heuristic judge',
  }
  const map = props.locale === 'zh' ? zh : en
  return map[value] || value
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

function isEvidenceExpanded(message) {
  return expandedEvidence.value.has(message.id)
}

// Group the evidence chain by (document, page): the chain is often very granular
// (many citations on the same page), which clutters the strip. One chip per page
// — clicking it highlights ALL of that page's evidence at once — is far cleaner.
function evidenceGroups(message) {
  const groups = []
  const byKey = new Map()
  for (const item of evidenceItems(message)) {
    const citation = resolveCitation(message, item)
    const documentId = citation?.documentId || ''
    const page = Number(item.page || citation?.page || 0)
    // Group same-document, same-page evidence together. Discovery-tool results
    // (trending / library) have no documentId and page 0; keying them by
    // citation id keeps each referenced paper its own chip instead of collapsing
    // every result into a single "::0" group.
    const key = documentId
      ? `${documentId}::${page}`
      : `nodoc::${item.citationId || citation?.id || groups.length}`
    let group = byKey.get(key)
    if (!group) {
      group = {
        key,
        documentId,
        page,
        // Identity reuses the page's FIRST real citation id, so the active-source
        // tracking (activeSourceDocId) and inline-marker highlighting still resolve.
        citationId: item.citationId,
        citationIds: [],
        bboxList: [],
        blockId: citation?.blockId || '',
        quote: item.quote || citation?.quote || '',
        source: item.source,
        sectionTitle: item.sectionTitle || '',
        crossDocName: citationCrossDocName(message, item),
      }
      byKey.set(key, group)
      groups.push(group)
    }
    group.citationIds.push(item.citationId)
    if (citation?.bboxList?.length) group.bboxList.push(...citation.bboxList)
    if (!group.sectionTitle && item.sectionTitle) group.sectionTitle = item.sectionTitle
  }
  return groups
}

// Only show evidence the user can act on: a chip is useful when clicking it jumps
// the reader (page anchor) or opens a referenced document. Discovery results like
// trending papers have no documentId and page 0 — they aren't clickable, so a strip
// of "trending paper (daily)" chips is just confusing noise. Drop them (and the
// whole strip hides when nothing actionable is left).
function isActionableEvidenceGroup(group) {
  return Boolean(group.documentId) || Number(group.page) > 0
}

function actionableEvidenceGroups(message) {
  return evidenceGroups(message).filter(isActionableEvidenceGroup)
}

// Collapsed preview: first-per-document guaranteed (so cross-doc pages aren't
// buried under "+N"), capped. Independent of expand state so the +N/Collapse
// toggle stays visible while expanded.
function evidenceGroupPreview(message) {
  const groups = actionableEvidenceGroups(message)
  const docs = answerSourceDocs(message)
  if (docs.length <= 1) return groups.slice(0, 6)
  const cap = Math.max(6, docs.length)
  const seen = new Set()
  const firstPerDoc = groups.filter((g) => {
    if (!g.documentId || seen.has(g.documentId)) return false
    seen.add(g.documentId)
    return true
  })
  const used = new Set()
  const out = []
  for (const g of [...firstPerDoc, ...groups]) {
    if (used.has(g.key)) continue
    used.add(g.key)
    out.push(g)
    if (out.length >= cap) break
  }
  return out
}

// Chips actually rendered: every page when expanded, else the preview.
function evidenceDisplayGroups(message) {
  return isEvidenceExpanded(message) ? actionableEvidenceGroups(message) : evidenceGroupPreview(message)
}

function evidenceHiddenCount(message) {
  return actionableEvidenceGroups(message).length - evidenceGroupPreview(message).length
}

function isEvidenceGroupActive(group) {
  return Boolean(props.activeCitationId) && group.citationIds.includes(props.activeCitationId)
}

// Highlight EVERY piece of evidence on the page at once: merge all the page's
// citation rects into one highlight payload and reuse the normal citation jump
// (which handles cross-document tab switching + painting).
function clickEvidenceGroup(message, group) {
  emit('citation-click', {
    id: group.citationId,
    documentId: group.documentId,
    page: group.page,
    blockId: group.blockId,
    bboxList: group.bboxList,
    quote: group.quote,
    source: group.source,
  })
}

function toggleEvidenceExpanded(message) {
  // Reassign a new Set so the ref's watchers fire (Set mutation alone wouldn't).
  const next = new Set(expandedEvidence.value)
  if (next.has(message.id)) next.delete(message.id)
  else next.add(message.id)
  expandedEvidence.value = next
}

function resolveCitation(message, evidence) {
  return (message.citations || []).find((citation) => citation.id === evidence.citationId) || null
}

// Resolve a user message's @-mentioned documents to display names for read-only
// chips. Accepts either the live (mentionedDocumentIds) or persisted
// (referencedDocumentIds) field so chips survive reload. Unknown ids fall back to
// the raw id so the provenance is never silently dropped.
function messageMentionNames(message) {
  const ids = message.mentionedDocumentIds || message.referencedDocumentIds || []
  if (!ids.length) return []
  return ids.map((id) => {
    const doc = (props.allDocuments || []).find((item) => item.id === id)
    return doc ? docDisplayName(doc) : id
  })
}

// When a citation belongs to an @-referenced document (not the one being read),
// return that document's short name so the evidence chip can badge its origin.
function citationCrossDocName(message, evidence) {
  const citation = resolveCitation(message, evidence)
  const docId = citation?.documentId
  if (!docId || docId === props.document.id) return ''
  const doc = (props.allDocuments || []).find((item) => item.id === docId)
  return doc ? docDisplayName(doc) : ''
}

// Distinct source documents an answer drew evidence from. Returns [] for a
// single-document answer (no clutter); for multi-document answers it lists each
// paper so the user can see the answer spans their library.
function answerSourceDocs(message) {
  const ids = [...new Set((message.citations || [])
    .map((citation) => citation.documentId)
    .filter(Boolean))]
  if (ids.length <= 1) return []
  return ids.map((id) => {
    const doc = (props.allDocuments || []).find((item) => item.id === id)
    return {
      id,
      name: doc ? docDisplayName(doc) : id,
      isFocus: id === props.document?.id,
    }
  })
}

// Clicking a SOURCES chip jumps to that document's first cited evidence (opening
// its tab first for cross-document citations, via the parent's citation handler),
// so the user can inspect a paper the answer drew on even when it isn't in focus.
function focusSourceDoc(message, src) {
  const citation = (message.citations || []).find((item) => item.documentId === src.id)
  if (citation) emit('citation-click', citation)
}

// The document that owns the currently-active citation, if any. Used to move the
// SOURCES highlight to whichever paper the user last clicked into, instead of
// always pinning it to the focus document.
function activeSourceDocId(message) {
  if (!props.activeCitationId) return null
  const citation = (message.citations || []).find(
    (item) => item.id === props.activeCitationId,
  )
  return citation?.documentId || null
}

// For a trace step that routed to another document via `documentId`, the target
// document's short name — so the user sees the agent fan out across papers.
function eventTargetDocName(event) {
  const docId = event?.tool?.args?.documentId
  if (!docId || docId === props.document?.id) return ''
  const doc = (props.allDocuments || []).find((item) => item.id === docId)
  return doc ? docDisplayName(doc) : docId
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
    literal: 'FTS',
    chat_history: 'Chat history',
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
    literal: '文本检索',
    chat_history: '对话历史',
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
      <div class="chat-header">
        <div class="session-tabs" @wheel="onSessionTabsWheel">
          <button
            v-for="tab in sessions"
            :key="tab.id"
            type="button"
            class="session-tab"
            :class="{ active: tab.active }"
            :title="tab.title"
            @mousedown.stop
            @click="emit('select-session', tab.id)"
          >
            <span class="session-tab-label">{{ tab.title }}</span>
            <span
              class="session-tab-close"
              role="button"
              :aria-label="ui.closeTab"
              @click.stop="emit('close-session', tab.id)"
            >×</span>
          </button>
          <span v-if="!sessions.length" class="session-tab placeholder">{{ ui.newSession }}</span>
        </div>
        <div class="chat-header-actions" @mousedown.stop>
          <button
            type="button"
            class="chat-icon-btn"
            :title="ui.newSession"
            :aria-label="ui.newSession"
            @click="emit('new-session')"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M12 5v14M5 12h14" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
            </svg>
          </button>
          <button
            type="button"
            class="chat-icon-btn"
            :class="{ active: historyOpen }"
            :title="ui.sessionHistory"
            :aria-label="ui.sessionHistory"
            @click="emit('toggle-history')"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" stroke-width="1.6" />
              <path d="M12 8v4l3 2" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
            </svg>
          </button>
          <button
            type="button"
            class="chat-icon-btn"
            :class="{ active: notesOpen }"
            :title="ui.notes"
            :aria-label="ui.notes"
            @click="emit('toggle-notes')"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M5 4h11l3 3v13H5V4Z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" />
              <path d="M8 9h8M8 13h8M8 17h5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
            </svg>
          </button>
        </div>
      </div>
      <div class="chat-subtitle-row" data-tauri-drag-region @mousedown="startWindowDrag">
        <span v-if="browsingLabel" class="chat-focus-label">{{ ui.browsing }}: {{ browsingLabel }}</span>
        <span v-else class="chat-focus-label">{{ ui.focusDoc }}: {{ document.shortTitle }}</span>
        <button
          v-if="focusDiffersFromView && !browsingLabel"
          type="button"
          class="focus-switch-btn"
          :title="`${ui.focusOnCurrent}: ${viewedDocName}`"
          @mousedown.stop
          @click="emit('set-focus-doc', viewedDocId)"
        >{{ ui.focusOnCurrent }}</button>
        <div class="subtitle-actions">
        <button
          type="button"
          class="chat-icon-btn subtitle-action-btn"
          :title="ui.exportChat"
          :aria-label="ui.exportChat"
          @mousedown.stop
          @click="emit('export-chat')"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 3v11m0 0 4-4m-4 4-4-4" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" />
            <path d="M5 20h14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" />
          </svg>
        </button>
        <button
          type="button"
          class="chat-icon-btn subtitle-action-btn"
          :disabled="!hasChatHistory"
          :title="ui.clearChatHistory"
          :aria-label="ui.clearChatHistory"
          @mousedown.stop
          @click="emit('clear-history')"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path
              d="M4 7h16M10 4h4a1 1 0 0 1 1 1v2H9V5a1 1 0 0 1 1-1Zm-3 3 1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M10 11v6M14 11v6"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
        </button>
        </div>
      </div>
      <div
        v-if="historyOpen"
        class="session-history-backdrop"
        @mousedown.stop="emit('close-history')"
      />
      <div v-if="historyOpen" class="session-history" @mousedown.stop>
        <div class="session-history-title">{{ ui.sessionHistory }}</div>
        <div class="session-history-list">
          <button
            v-for="item in historyItems"
            :key="item.id"
            type="button"
            class="session-history-row"
            :class="{ active: item.active }"
            @click="emit('select-session', item.id)"
          >
            <span class="session-history-label">{{ item.title }}</span>
            <span v-if="item.focusTitle" class="session-history-meta">{{ item.focusTitle }}</span>
            <span
              class="session-history-del"
              role="button"
              :aria-label="ui.deleteSession"
              @click.stop="emit('delete-session', item.id)"
            >×</span>
          </button>
          <div v-if="!historyItems.length" class="session-history-empty">{{ ui.chatEmptyHint }}</div>
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
            <div
              v-if="message.role === 'user' && messageMentionNames(message).length"
              class="message-mentions"
            >
              <span
                v-for="(name, index) in messageMentionNames(message)"
                :key="`${message.id}-mention-${index}`"
                class="message-mention-chip"
                :title="name"
              >@{{ name }}</span>
            </div>
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
            <!-- Inline edit of the last user question: textarea replaces the bubble. -->
            <div v-if="editingMessageId === message.id" class="message-edit">
              <textarea
                v-model="editDraft"
                class="message-edit-input"
                rows="2"
                :placeholder="ui.inputPlaceholder"
                @keydown.enter.exact.prevent="submitEdit(message)"
                @keydown.esc.prevent="cancelEdit()"
                @mousedown.stop
              ></textarea>
              <div class="message-edit-actions">
                <button type="button" class="message-edit-cancel" @click="cancelEdit()">{{ ui.cancel }}</button>
                <button
                  type="button"
                  class="message-edit-submit"
                  :disabled="!editDraft.trim()"
                  @click="submitEdit(message)"
                >{{ ui.reAsk }}</button>
              </div>
            </div>
            <MarkdownText
              v-else-if="messageText(message)"
              class="message-content"
              :text="messageText(message)"
              :loading="message.role === 'assistant' && message.status === 'running'"
            />
            <div
              v-if="isEditableUserMessage(message) && editingMessageId !== message.id"
              class="message-edit-affordance"
            >
              <button
                type="button"
                class="message-edit-btn"
                :title="ui.editAndReask"
                :aria-label="ui.editAndReask"
                @click="startEdit(message)"
              >
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d="M4 20h4L18 10l-4-4L4 16v4Z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" />
                  <path d="M13.5 6.5l4 4" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
                </svg>
                <span>{{ ui.editAndReask }}</span>
              </button>
            </div>
            <div v-else-if="isWaitingForAnswer(message) && !agentProcessVisible(message)" class="message-loading" :aria-label="runningStatusLabel(message)">
              <span class="loading-dots" aria-hidden="true">
                <span></span>
                <span></span>
                <span></span>
              </span>
              <span>{{ runningStatusLabel(message) }}</span>
            </div>
            <div v-if="message.provider" class="message-provider">{{ message.provider }}</div>

            <div v-if="actionableEvidenceGroups(message).length" class="evidence-group">
              <div v-if="answerSourceDocs(message).length" class="evidence-sources">
                <span class="evidence-sources-label">{{ ui.sources }}</span>
                <button
                  v-for="src in answerSourceDocs(message)"
                  :key="`${message.id}-src-${src.id}`"
                  class="evidence-source-chip"
                  :class="{
                    focus: src.isFocus && activeSourceDocId(message) === null,
                    active: activeSourceDocId(message) === src.id,
                  }"
                  :title="src.name"
                  @click="focusSourceDoc(message, src)"
                >{{ src.name }}</button>
              </div>
              <div class="evidence-strip">
                <div class="evidence-strip-head">
                  <span class="evidence-strip-label">{{ ui.evidence }}</span>
                  <button
                    v-if="evidenceHiddenCount(message) > 0"
                    type="button"
                    class="evidence-more"
                    :aria-expanded="isEvidenceExpanded(message)"
                    @click="toggleEvidenceExpanded(message)"
                  >{{ isEvidenceExpanded(message) ? ui.evidenceCollapse : `+${evidenceHiddenCount(message)}` }}</button>
                </div>
                <div class="evidence-chips">
                  <button
                    v-for="group in evidenceDisplayGroups(message)"
                    :key="`${message.id}-grp-${group.key}`"
                    class="evidence-chip"
                    :class="{ active: isEvidenceGroupActive(group) }"
                    :title="group.quote"
                    @click="clickEvidenceGroup(message, group)"
                  >
                    <span
                      v-if="group.crossDocName"
                      class="evidence-doc-badge"
                      :title="group.crossDocName"
                    >@{{ group.crossDocName }}</span>
                    <span v-if="group.page > 0" class="evidence-chip-page">{{ locale === 'zh' ? `${ui.page}${group.page}` : `p${group.page}` }}</span>
                    <span class="evidence-chip-title">{{ group.sectionTitle || evidenceSourceLabel(group.source) }}</span>
                    <span v-if="group.citationIds.length > 1" class="evidence-chip-count">×{{ group.citationIds.length }}</span>
                  </button>
                </div>
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
                    <span>{{ formatRuntime(traceJudgeDetails(message).runtime) }}</span>
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
                      <span>
                        {{ eventTitle(event) }}
                        <span v-if="eventTargetDocName(event)" class="agent-step-doc">{{ eventTargetDocName(event) }}</span>
                      </span>
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

        <div v-if="mentionedDocs.length" class="mention-chips" v-bind="testAttrs('chat-mention-chips')">
          <span
            v-for="doc in mentionedDocs"
            :key="doc.id"
            class="mention-chip"
            v-bind="testAttrs('chat-mention-chip')"
          >
            <span class="mention-chip-at">@</span>
            <span class="mention-chip-name">{{ docDisplayName(doc) }}</span>
            <button
              type="button"
              class="mention-chip-remove"
              :title="ui.mentionPaperRemove"
              :aria-label="ui.mentionPaperRemove"
              @click="removeMention(doc.id)"
            >×</button>
          </span>
        </div>

        <div v-if="mentionPickerOpen" class="mention-picker" v-bind="testAttrs('chat-mention-picker')">
          <input
            ref="mentionSearchRef"
            v-model="mentionFilter"
            class="mention-picker-search"
            type="text"
            :placeholder="ui.mentionPaperPlaceholder"
            @keydown="handleMentionPickerKeydown"
            @blur="closeMentionPicker()"
          />
          <ul v-if="mentionCandidates.length" class="mention-picker-list">
            <li
              v-for="(doc, index) in mentionCandidates"
              :key="doc.id"
              class="mention-picker-item"
              :class="{ active: index === mentionActiveIndex }"
              v-bind="testAttrs('chat-mention-option')"
              @mousedown.prevent="selectMention(doc.id)"
              @mouseenter="mentionActiveIndex = index"
            >
              {{ docDisplayName(doc) }}
            </li>
          </ul>
          <div v-else class="mention-picker-empty">{{ ui.mentionPaperNotFound }}</div>
        </div>

        <textarea
          ref="composerTextareaRef"
          :disabled="!chatInputEnabled"
          :placeholder="
            !modelConfigured
              ? ui.modelNotConfiguredHint
              : supportsVision
                ? ui.imageInputPlaceholder
                : ui.inputPlaceholder
          "
          @keydown="handleComposerKeydown"
          @input="handleComposerInput"
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

            <button
              v-if="props.runningEventId || lastTurnRunning"
              class="submit-btn stop"
              type="button"
              :title="ui.stopGeneration"
              :aria-label="ui.stopGeneration"
              @click="emit('stop-generation', props.runningEventId || runningActivityEventId)"
            >
              <span class="icon-stop" aria-hidden="true"></span>
            </button>
            <button v-else class="submit-btn" :disabled="!chatInputEnabled" type="submit" :title="ui.sendMessage" :aria-label="ui.sendMessage">
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
  top: 12px;
  left: -12px;
  width: 24px;
  height: 24px;
  border-radius: 999px;
  border: 1px solid var(--line-soft);
  background: var(--bg-elevated);
  color: var(--text-secondary);
  cursor: pointer;
  z-index: 3;
  font-size: 11px;
  transition: color 140ms ease, background 140ms ease;
}

.collapse-btn:hover {
  color: var(--text-primary);
  background: rgba(255, 255, 255, 0.05);
}

.collapsed-rail {
  flex: 1;
  width: 100%;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  writing-mode: vertical-rl;
  padding: 16px 0;
  font-size: 12px;
  letter-spacing: 2px;
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
  position: relative;
  padding: 12px 14px 10px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-shrink: 0;
}

.chat-subtitle-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 16px 12px;
  border-bottom: 1px solid var(--line-soft);
  color: var(--text-muted);
  font-size: 12px;
  flex-shrink: 0;
}

.chat-focus-label {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.focus-switch-btn {
  flex-shrink: 0;
  border: 1px solid rgba(106, 169, 255, 0.34);
  border-radius: 999px;
  padding: 2px 9px;
  background: rgba(106, 169, 255, 0.1);
  color: var(--text-secondary);
  font-size: 11px;
  cursor: pointer;
  white-space: nowrap;
  transition: color 140ms ease, background 140ms ease;
}

.focus-switch-btn:hover {
  color: var(--text-primary);
  background: rgba(106, 169, 255, 0.18);
}

/* Clear-history button lives at the far right of the Focus row. */
/* Right-aligned cluster of per-conversation actions (export, clear) kept adjacent. */
.subtitle-actions {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.subtitle-action-btn {
  width: 24px;
  height: 24px;
  flex-shrink: 0;
}

.subtitle-action-btn svg {
  width: 13px;
  height: 13px;
}

/* Session tabs ---------------------------------------------------------- */
.session-tabs {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
  flex: 1;
  /* `scroll` (not `auto`) keeps the 8px scrollbar lane reserved at all times.
     With a styled (classic) webkit scrollbar, `auto` only reserves the lane
     when content overflows, which grew the strip's height the moment tabs
     overflowed and bumped the centered header icons down a couple px. A always
     reserved lane keeps the row height constant. */
  /* `scroll` keeps the 8px scrollbar lane reserved at all times (see comment
     above). Scrollbar look comes from the global style in styles/main.css. */
  overflow-x: scroll;
}

.session-tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 150px;
  padding: 4px 8px 4px 10px;
  border: 1px solid var(--line-soft);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.02);
  color: var(--text-muted);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  flex-shrink: 0;
  transition: border-color 140ms ease, color 140ms ease, background 140ms ease;
}

.session-tab:hover {
  color: var(--text-secondary);
}

.session-tab.active {
  background: rgba(106, 169, 255, 0.14);
  border-color: rgba(106, 169, 255, 0.34);
  color: var(--text-primary);
}

.session-tab.placeholder {
  cursor: default;
  opacity: 0.6;
}

.session-tab-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.session-tab-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 15px;
  height: 15px;
  border-radius: 4px;
  font-size: 13px;
  line-height: 1;
  color: var(--text-muted);
  opacity: 0.6;
}

.session-tab-close:hover {
  opacity: 1;
  background: rgba(255, 255, 255, 0.08);
}

/* Header icon group ----------------------------------------------------- */
.chat-icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: 1px solid var(--line-soft);
  border-radius: 999px;
  padding: 0;
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-secondary);
  cursor: pointer;
  transition: border-color 140ms ease, color 140ms ease, background 140ms ease;
}

.chat-icon-btn svg {
  width: 15px;
  height: 15px;
}

.chat-icon-btn:hover:not(:disabled) {
  border-color: rgba(106, 169, 255, 0.34);
  color: var(--text-primary);
  background: rgba(106, 169, 255, 0.08);
}

.chat-icon-btn.active {
  border-color: rgba(106, 169, 255, 0.45);
  color: var(--text-primary);
  background: rgba(106, 169, 255, 0.16);
}

.chat-icon-btn:disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

/* Session history dropdown --------------------------------------------- */
.session-history-backdrop {
  position: absolute;
  inset: 0;
  z-index: 30;
}

.session-history {
  position: absolute;
  top: 46px;
  right: 12px;
  z-index: 31;
  width: 260px;
  max-height: 320px;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--line-soft);
  border-radius: 10px;
  background: var(--bg-panel, #1b1d22);
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.4);
  overflow: hidden;
}

.session-history-title {
  padding: 10px 12px 6px;
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--text-muted);
}

.session-history-list {
  overflow-y: auto;
  padding: 0 6px 8px;
}

.session-history-row {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 8px 8px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  text-align: left;
}

.session-history-row:hover {
  background: rgba(255, 255, 255, 0.05);
}

.session-history-row.active {
  background: rgba(106, 169, 255, 0.14);
  color: var(--text-primary);
}

.session-history-label {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.session-history-meta {
  font-size: 11px;
  color: var(--text-muted);
  max-width: 90px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.session-history-del {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 5px;
  font-size: 14px;
  line-height: 1;
  color: var(--text-muted);
  opacity: 0.6;
}

.session-history-del:hover {
  opacity: 1;
  color: #ffd2d2;
  background: rgba(255, 99, 99, 0.1);
}

.session-history-empty {
  padding: 12px;
  font-size: 12px;
  color: var(--text-muted);
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

.pane-tabs {
  display: inline-flex;
  gap: 4px;
  padding: 3px;
  border: 1px solid var(--line-soft);
  border-radius: 999px;
  flex-shrink: 0;
}

.pane-tabs button {
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 12px;
  font-weight: 600;
  padding: 3px 10px;
  border-radius: 999px;
}

.pane-tabs button:hover {
  color: var(--text-secondary);
}

.pane-tabs button.active {
  background: rgba(106, 169, 255, 0.14);
  color: var(--text-primary);
  cursor: default;
}

.chat-header-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
  /* The tab strip carries an 8px scrollbar lane at its bottom, which biases its
     visible tabs upward under align-items:center. Give the icon group the same
     8px bottom reserve so both columns' centers land on one line. Keep in sync
     with .session-tabs::-webkit-scrollbar height. */
  margin-bottom: 8px;
}

.chat-clear-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: 1px solid var(--line-soft);
  border-radius: 999px;
  padding: 0;
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-secondary);
  cursor: pointer;
  transition: border-color 140ms ease, color 140ms ease, background 140ms ease;
}

.chat-clear-btn .clear-icon {
  width: 15px;
  height: 15px;
}

.chat-clear-btn:hover:not(:disabled) {
  border-color: rgba(255, 179, 179, 0.34);
  color: #ffd2d2;
  background: rgba(255, 99, 99, 0.08);
}

.chat-clear-btn:disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

.message-list {
  position: relative;
  flex: 1;
  min-height: 0;
  /* Scrollbar look (slim, transparent, hover/.is-scrolling reveal) comes from
     the global style in styles/main.css. Do not set scrollbar-width here — it
     makes WebKit ignore the global ::-webkit-scrollbar rules. */
  overflow: auto;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
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
  position: relative;
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

/* Inline "edit & re-ask" of the last user question. */
.message-edit {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.message-edit-input {
  width: 100%;
  resize: vertical;
  min-height: 44px;
  padding: 8px 10px;
  border: 1px solid rgba(106, 169, 255, 0.4);
  border-radius: 10px;
  background: var(--bg-app);
  color: var(--text-primary);
  font: inherit;
  font-size: 13px;
  line-height: 1.5;
  outline: none;
}

.message-edit-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.message-edit-cancel,
.message-edit-submit {
  border-radius: 999px;
  padding: 4px 12px;
  font-size: 12px;
  cursor: pointer;
  border: 1px solid var(--line-soft);
  background: transparent;
  color: var(--text-secondary);
}

.message-edit-submit {
  border-color: rgba(106, 169, 255, 0.45);
  background: rgba(106, 169, 255, 0.16);
  color: var(--text-primary);
}

.message-edit-submit:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.message-edit-affordance {
  /* Pinned to the card's top-right corner, aligned with the "USER" label row. */
  position: absolute;
  top: 10px;
  right: 12px;
  display: flex;
  margin: 0;
}

.message-edit-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 11px;
  padding: 2px 4px;
  border-radius: 6px;
  cursor: pointer;
  opacity: 0.75;
  transition: opacity 140ms ease, color 140ms ease, background 140ms ease;
}

.message-edit-btn svg {
  width: 13px;
  height: 13px;
}

.message-edit-btn:hover {
  opacity: 1;
  color: var(--text-secondary);
  background: rgba(255, 255, 255, 0.05);
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

.message-mentions {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-bottom: 8px;
}

.message-mention-chip {
  max-width: 160px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
  color: var(--accent, #6aa6ff);
  background: rgba(120, 170, 255, 0.14);
  border-radius: 6px;
  padding: 1px 6px;
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

/* Multi-document "Sources:" line above the evidence strip. */
.evidence-sources {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 8px;
}

.evidence-sources-label {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.evidence-source-chip {
  padding: 2px 8px;
  border: 1px solid var(--line-soft);
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-secondary);
  font-size: 11px;
  max-width: 160px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
  font-family: inherit;
  transition:
    border-color 0.12s ease,
    background 0.12s ease,
    color 0.12s ease;
}

.evidence-source-chip:hover {
  border-color: rgba(106, 169, 255, 0.45);
  color: var(--text-primary);
}

.evidence-source-chip.focus {
  border-color: rgba(106, 169, 255, 0.34);
  background: rgba(106, 169, 255, 0.12);
  color: var(--text-primary);
}

/* Active = the source document the user clicked into (owns the active citation).
   Gold, matching the active evidence chip, so "selected" reads the same way in
   both rows. */
.evidence-source-chip.active {
  border-color: rgba(250, 204, 21, 0.45);
  background: rgba(250, 204, 21, 0.12);
  color: var(--text-primary);
}

.evidence-strip {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

/* Header row: the EVIDENCE label on the left, the expand/collapse toggle on the
   right. Keeping the toggle here (not in the chip grid) avoids it landing alone
   on its own row when the preview chip count is even. */
.evidence-strip-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.evidence-strip-label {
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

/* Auto-fill grid: chips become equal-width columns that fill the row edge to
   edge, so the right side is no longer left empty. Falls to fewer columns as
   the pane narrows. */
.evidence-chips {
  flex: 1;
  min-width: 0;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 6px;
  align-content: start;
}

.evidence-chip {
  display: inline-flex;
  align-items: center;
  min-width: 0;
  min-height: 28px;
  gap: 5px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.035);
  color: var(--text-secondary);
  cursor: pointer;
  padding: 0 10px;
  font-size: 11px;
}

.evidence-chip-page {
  flex-shrink: 0;
}

/* Per-page evidence count ("×8"): a subtle pill at the chip's tail. */
.evidence-chip-count {
  flex-shrink: 0;
  padding: 1px 6px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-muted);
  font-size: 10px;
  font-variant-numeric: tabular-nums;
}

/* The section title is the only flexible part: it takes the remaining cell width
   and truncates, so every chip stays within its grid column. */
.evidence-chip-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.evidence-chip:hover,
.evidence-chip.active {
  border-color: rgba(245, 180, 24, 0.4);
  background: rgba(245, 180, 24, 0.12);
  color: var(--text-primary);
}

.evidence-more {
  display: inline-flex;
  align-items: center;
  flex-shrink: 0;
  height: 22px;
  padding: 0 10px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-secondary);
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
  transition: border-color 140ms ease, background 140ms ease, color 140ms ease;
}

.evidence-more:hover {
  border-color: rgba(245, 180, 24, 0.4);
  background: rgba(245, 180, 24, 0.12);
  color: var(--text-primary);
}

.evidence-doc-badge {
  max-width: 90px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--accent, #6aa6ff);
  background: rgba(120, 170, 255, 0.14);
  border-radius: 6px;
  padding: 0 5px;
  font-size: 10px;
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

/* "in <Doc>" badge on a trace step that searched another document. */
.agent-step-doc {
  display: inline-block;
  margin-left: 6px;
  padding: 1px 6px;
  border-radius: 999px;
  background: rgba(106, 169, 255, 0.14);
  color: var(--text-secondary);
  font-size: 10px;
  font-weight: 600;
  vertical-align: middle;
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

.mention-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 8px;
}

.mention-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 6px 3px 8px;
  border-radius: 999px;
  background: rgba(120, 170, 255, 0.14);
  border: 1px solid rgba(120, 170, 255, 0.32);
  color: var(--text-primary);
  font-size: 12px;
  max-width: 100%;
}

.mention-chip-at {
  color: var(--accent, #6aa6ff);
  font-weight: 600;
}

.mention-chip-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 180px;
}

.mention-chip-remove {
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
  padding: 0 2px;
}

.mention-picker {
  margin-bottom: 8px;
  border-radius: 14px;
  border: 1px solid var(--line-soft);
  background: var(--surface-raised, rgba(20, 24, 32, 0.96));
  overflow: hidden;
}

.mention-picker-search {
  width: 100%;
  border: none;
  border-bottom: 1px solid var(--line-soft);
  background: transparent;
  color: var(--text-primary);
  padding: 10px 12px;
  outline: none;
  font-size: 13px;
}

.mention-picker-list {
  list-style: none;
  margin: 0;
  padding: 4px;
  max-height: 180px;
  overflow-y: auto;
}

.mention-picker-item {
  padding: 8px 10px;
  border-radius: 8px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mention-picker-item.active {
  background: rgba(120, 170, 255, 0.16);
}

.mention-picker-empty {
  padding: 12px;
  color: var(--text-secondary);
  font-size: 12px;
  text-align: center;
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

.icon-stop {
  display: inline-block;
  width: 11px;
  height: 11px;
  border-radius: 2px;
  background: currentColor;
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
