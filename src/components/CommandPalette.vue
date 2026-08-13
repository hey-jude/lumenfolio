<script setup>
import { computed, nextTick, ref, watch } from 'vue'
import UiField from './ui/UiField.vue'
import UiRow from './ui/UiRow.vue'
import UiChip from './ui/UiChip.vue'
import UiEmpty from './ui/UiEmpty.vue'

/**
 * Cmd/Ctrl+K — one entry point for the things that were scattered across the
 * rail icons, the add menu and the sidebar's right-click menu.
 *
 * It searches sources by title only. Searching their *contents* is what the
 * chat composer is for, and blurring those two would leave the user unsure
 * which one they were using.
 */
const props = defineProps({
  open: { type: Boolean, default: false },
  /** [{ id, title, shortTitle, indexStatus }] */
  documents: { type: Array, default: () => [] },
  /** [{ id, label, hint, kind }] — supplied by the host so this stays dumb. */
  actions: { type: Array, default: () => [] },
  ui: { type: Object, required: true },
})

const emit = defineEmits(['close', 'run-action', 'open-doc'])

const query = ref('')
const cursor = ref(0)
const inputRef = ref(null)
const listRef = ref(null)

const MAX_DOCS = 8

const normalized = computed(() => query.value.trim().toLowerCase())

const matchedActions = computed(() => {
  if (!normalized.value) return props.actions
  return props.actions.filter((action) =>
    `${action.label} ${action.hint || ''}`.toLowerCase().includes(normalized.value),
  )
})

const matchedDocs = computed(() => {
  const docs = props.documents
  // With no query this is a recents list, not a search result — showing the
  // whole library unprompted would bury the actions under it.
  const pool = normalized.value
    ? docs.filter((doc) =>
        String(doc.shortTitle || doc.title || '')
          .toLowerCase()
          .includes(normalized.value),
      )
    : docs
  return pool.slice(0, MAX_DOCS)
})

// One flat list so the keyboard cursor can run straight through both groups.
const items = computed(() => [
  ...matchedActions.value.map((action) => ({ type: 'action', key: `a:${action.id}`, action })),
  ...matchedDocs.value.map((doc) => ({ type: 'doc', key: `d:${doc.id}`, doc })),
])

watch(
  () => props.open,
  async (open) => {
    if (!open) return
    query.value = ''
    cursor.value = 0
    await nextTick()
    inputRef.value?.focus()
  },
)

// Any change to the result set can leave the cursor past the end.
watch(items, (list) => {
  if (cursor.value > list.length - 1) cursor.value = Math.max(0, list.length - 1)
})

function move(step) {
  const total = items.value.length
  if (!total) return
  cursor.value = (cursor.value + step + total) % total
  nextTick(() => {
    listRef.value
      ?.querySelector('[data-cursor="true"]')
      ?.scrollIntoView({ block: 'nearest' })
  })
}

function run(item) {
  if (!item) return
  if (item.type === 'action') emit('run-action', item.action.id)
  else emit('open-doc', item.doc.id)
  emit('close')
}

function onKeydown(event) {
  if (event.key === 'ArrowDown') {
    event.preventDefault()
    move(1)
  } else if (event.key === 'ArrowUp') {
    event.preventDefault()
    move(-1)
  } else if (event.key === 'Enter') {
    event.preventDefault()
    run(items.value[cursor.value])
  } else if (event.key === 'Escape') {
    event.preventDefault()
    emit('close')
  }
}

function docLabel(doc) {
  return doc.shortTitle || doc.title || ''
}
</script>

<template>
  <div v-if="props.open" class="palette-backdrop" @click.self="emit('close')">
    <div class="palette" role="dialog" aria-modal="true" :aria-label="props.ui.commandPaletteTitle">
      <div class="palette-search">
        <UiField size="md">
          <template #leading>
            <svg viewBox="0 0 16 16" fill="none" aria-hidden="true" class="palette-search-icon">
              <circle cx="7" cy="7" r="4.25" stroke="currentColor" stroke-width="1.5" />
              <path d="m10.5 10.5 3 3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
            </svg>
          </template>
          <input
            ref="inputRef"
            v-model="query"
            type="text"
            :placeholder="props.ui.commandPalettePlaceholder"
            @keydown="onKeydown"
          />
        </UiField>
      </div>

      <div v-if="items.length" ref="listRef" class="palette-list" role="listbox">
        <template v-for="(item, index) in items" :key="item.key">
          <!-- Headings are interleaved rather than wrapping each group in its own
               list, so one cursor index still walks straight through both. -->
          <p v-if="index === 0 && item.type === 'action'" class="palette-group">
            {{ props.ui.commandPaletteActions }}
          </p>
          <p v-if="item.type === 'doc' && index === matchedActions.length" class="palette-group">
            {{ normalized ? props.ui.commandPaletteSources : props.ui.commandPaletteRecent }}
          </p>
          <UiRow
            size="md"
            role="option"
            :active="index === cursor"
            :data-cursor="index === cursor"
            :aria-selected="index === cursor"
            @click="run(item)"
          >
            <template #leading>
              <svg v-if="item.type === 'action'" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                <path d="M8 3.5v9M3.5 8h9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
              </svg>
              <svg v-else viewBox="0 0 16 16" fill="none" aria-hidden="true">
                <path d="M4 2.5h5l3 3v8h-8z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" />
              </svg>
            </template>
            <span class="palette-label">
              {{ item.type === 'action' ? item.action.label : docLabel(item.doc) }}
            </span>
            <template #trailing>
              <UiChip
                v-if="item.type === 'action' && item.action.hint"
                tone="neutral"
                size="sm"
              >{{ item.action.hint }}</UiChip>
              <UiChip
                v-else-if="item.type === 'doc' && item.doc.indexStatus === 'indexing'"
                tone="warning"
                size="sm"
              >{{ props.ui.commandPaletteIndexing }}</UiChip>
            </template>
          </UiRow>
        </template>
      </div>

      <div v-else class="palette-blank">
        <UiEmpty :title="props.ui.commandPaletteEmpty" :copy="props.ui.commandPaletteEmptyHint" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.palette-backdrop {
  position: fixed;
  inset: 0;
  z-index: 1500;
  display: flex;
  justify-content: center;
  /* flex-start, not the default stretch: without it the dialog is pulled to the
     full height of the backdrop and five actions sit above a tall empty box. */
  align-items: flex-start;
  /* Anchored high rather than centered: the list grows downward, and a centered
     dialog would jump as results come and go. */
  padding-top: 14vh;
  background: var(--scrim);
}

.palette {
  width: min(560px, calc(100vw - 32px));
  max-height: 60vh;
  display: flex;
  flex-direction: column;
  border-radius: var(--r-lg);
  background: var(--surface-3);
  box-shadow: var(--shadow-overlay);
  overflow: hidden;
}

.palette-search {
  padding: var(--gap-4);
  border-bottom: 1px solid var(--line);
}

.palette-search-icon {
  width: 14px;
  height: 14px;
}

.palette-list {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: var(--gap-2);
}

.palette-group {
  margin: var(--gap-2) var(--gap-3) var(--gap-1);
  color: var(--ink-3);
  font-size: var(--fs-micro);
  font-weight: var(--w-medium);
  letter-spacing: var(--tracking-caps);
  text-transform: uppercase;
}

.palette-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--fs-body-lg);
}

.palette-blank {
  padding: var(--gap-6) var(--gap-5);
}
</style>
