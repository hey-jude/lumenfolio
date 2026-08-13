<script setup>
import { computed } from 'vue'
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
  // When rendered as the floating drawer (over the Agent pane), the header shows
  // a single close affordance instead of the Chat|Notes tab toggle.
  asDrawer: {
    type: Boolean,
    default: false,
  },
  width: {
    type: Number,
    default: 420,
  },
  notes: {
    type: Array,
    default: () => [],
  },
  loading: {
    type: Boolean,
    default: false,
  },
  activeNoteId: {
    type: String,
    default: '',
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
  'set-tab',
  'note-focus',
  'note-edit',
  'note-delete',
])

const sortedNotes = computed(() => props.notes || [])

function quotePreview(note) {
  const text = String(note?.quoteText || '').replace(/\s+/g, ' ').trim()
  if (!text) return ''
  return text.length > 140 ? `${text.slice(0, 140)}...` : text
}

function timeLabel(note) {
  const ts = Number(note?.updatedAt || note?.createdAt || 0)
  if (!ts) return ''
  try {
    return new Date(ts * 1000).toLocaleString(props.locale === 'zh' ? 'zh-CN' : 'en-US', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    })
  } catch {
    return ''
  }
}
</script>

<template>
  <aside class="notes-shell" :style="{ width: collapsed ? '44px' : `${width}px` }" :class="{ collapsed }">
    <button v-if="!asDrawer" class="collapse-btn" type="button" @click="emit('toggle-collapse')">
      {{ collapsed ? '❮' : '❯' }}
    </button>

    <button
      v-if="collapsed"
      type="button"
      class="collapsed-rail"
      :aria-label="ui.notes"
      @click="emit('toggle-collapse')"
    >
      <span>{{ ui.notes }}</span>
    </button>

    <template v-else>
      <div class="notes-header" data-tauri-drag-region @mousedown="startWindowDrag">
        <div>
          <div class="notes-title">{{ ui.notes }}</div>
          <div class="notes-subtitle">{{ ui.currentDoc }}: {{ document.shortTitle }}</div>
        </div>
        <button
          v-if="asDrawer"
          type="button"
          class="notes-close-btn"
          :title="ui.close || ui.collapse"
          :aria-label="ui.close || ui.collapse"
          @click="emit('toggle-collapse')"
        >✕</button>
        <div v-else class="pane-tabs">
          <button type="button" @click="emit('set-tab', 'chat')">{{ ui.chat }}</button>
          <button type="button" class="active">{{ ui.notes }}</button>
        </div>
      </div>

      <div class="notes-body">
        <div v-if="loading" class="notes-status">{{ ui.notesLoading }}</div>

        <div v-else-if="!sortedNotes.length" class="notes-empty">
          <div class="notes-empty-title">{{ ui.notesEmptyTitle }}</div>
          <div class="notes-empty-copy">{{ ui.notesEmptyHint }}</div>
        </div>

        <div v-else class="notes-list">
          <article
            v-for="note in sortedNotes"
            :key="note.id"
            class="note-card"
            :class="{ active: note.id === activeNoteId }"
            @click="emit('note-focus', note)"
          >
            <div class="note-card-head">
              <span class="note-page">{{ ui.page }} {{ note.page }}</span>
              <span class="note-time">{{ timeLabel(note) }}</span>
            </div>
            <div v-if="quotePreview(note)" class="note-quote">"{{ quotePreview(note) }}"</div>
            <div v-if="note.content" class="note-content">
              <MarkdownText :text="note.content" />
            </div>
            <div class="note-actions" @click.stop>
              <button type="button" @click="emit('note-edit', note)">{{ ui.edit }}</button>
              <button type="button" class="danger" @click="emit('note-delete', note)">{{ ui.delete }}</button>
            </div>
          </article>
        </div>
      </div>
    </template>
  </aside>
</template>

<style scoped>
.notes-shell {
  position: relative;
  display: flex;
  flex-direction: column;
  height: 100%;
  border-left: 1px solid var(--line);
  background: var(--surface-1);
  transition: width var(--dur-slow) var(--ease);
}

.notes-shell.collapsed {
  align-items: center;
}

.collapse-btn {
  position: absolute;
  top: 12px;
  left: -12px;
  width: 24px;
  height: 24px;
  border-radius: var(--r-pill);
  border: 1px solid var(--line);
  background: var(--surface-2);
  color: var(--ink-2);
  cursor: pointer;
  z-index: 3;
  font-size: 11px;
  transition: color var(--dur-base) var(--ease), background var(--dur-base) var(--ease);
}

.collapse-btn:hover {
  color: var(--ink);
  background: var(--surface-hover);
}

.collapsed-rail {
  flex: 1;
  width: 100%;
  border: none;
  background: transparent;
  color: var(--ink-2);
  cursor: pointer;
  writing-mode: vertical-rl;
  padding: 16px 0;
  font-size: 12px;
  letter-spacing: 2px;
}

.collapsed-rail:hover {
  color: var(--ink);
  background: var(--surface-wash);
}

.notes-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 18px 18px 14px;
  border-bottom: 1px solid var(--line);
  flex-shrink: 0;
}

.notes-close-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: 1px solid var(--line);
  border-radius: var(--r-pill);
  padding: 0;
  background: var(--surface-wash);
  color: var(--ink-2);
  cursor: pointer;
  font-size: 13px;
  flex-shrink: 0;
  transition: border-color var(--dur-base) var(--ease), color var(--dur-base) var(--ease), background var(--dur-base) var(--ease);
}

.notes-close-btn:hover {
  border-color: var(--accent-line);
  color: var(--ink);
  background: var(--accent-tint);
}

.notes-title {
  font-size: 16px;
  font-weight: var(--w-strong);
  color: var(--ink);
}

.notes-subtitle {
  margin-top: 4px;
  font-size: 12px;
  color: var(--ink-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 220px;
}

.pane-tabs {
  display: inline-flex;
  gap: 4px;
  padding: 3px;
  border: 1px solid var(--line);
  border-radius: var(--r-pill);
  flex-shrink: 0;
}

.pane-tabs button {
  border: none;
  background: transparent;
  color: var(--ink-3);
  cursor: pointer;
  font-size: 12px;
  font-weight: var(--w-strong);
  padding: 3px 10px;
  border-radius: var(--r-pill);
}

.pane-tabs button:hover {
  color: var(--ink-2);
}

.pane-tabs button.active {
  background: var(--accent-tint);
  color: var(--ink);
  cursor: default;
}

.notes-body {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 12px;
}

.notes-status,
.notes-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  height: 100%;
  color: var(--ink-3);
  text-align: center;
}

.notes-empty-title {
  font-size: 13px;
  font-weight: var(--w-strong);
  color: var(--ink-2);
}

.notes-empty-copy {
  font-size: 12px;
  max-width: 240px;
  line-height: 1.5;
}

.notes-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.note-card {
  border: 1px solid var(--line);
  border-radius: var(--r-md);
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  cursor: pointer;
  background: var(--surface-wash);
  transition: border-color var(--dur-base) var(--ease), background var(--dur-base) var(--ease);
}

.note-card:hover {
  border-color: var(--accent-line);
}

.note-card.active {
  border-color: var(--accent-line);
  background: var(--accent-tint);
}

.note-card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.note-page {
  font-size: 11px;
  font-weight: var(--w-strong);
  color: var(--accent);
  background: var(--accent-tint);
  padding: 1px 8px;
  border-radius: var(--r-pill);
}

.note-time {
  font-size: 11px;
  color: var(--ink-3);
}

.note-quote {
  font-size: 12px;
  color: var(--ink-3);
  line-height: 1.45;
  border-left: 2px solid var(--line);
  padding-left: 8px;
}

.note-content {
  font-size: 13px;
  color: var(--ink);
  line-height: 1.5;
  word-break: break-word;
}

.note-actions {
  display: flex;
  justify-content: flex-end;
  gap: 6px;
}

.note-actions button {
  border: none;
  background: transparent;
  color: var(--ink-3);
  cursor: pointer;
  font-size: 12px;
  padding: 2px 6px;
  border-radius: var(--r-sm);
}

.note-actions button:hover {
  color: var(--ink);
  background: var(--surface-hover);
}

.note-actions button.danger:hover {
  color: var(--danger);
  background: var(--danger-tint);
}
</style>
