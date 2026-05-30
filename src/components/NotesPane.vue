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
    <button class="collapse-btn" type="button" @click="emit('toggle-collapse')">
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
        <div class="pane-tabs">
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
  border-left: 1px solid var(--line-soft);
  background: var(--bg-panel);
  transition: width 0.18s ease;
  overflow: hidden;
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

.notes-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 18px 18px 14px;
  border-bottom: 1px solid var(--line-soft);
  flex-shrink: 0;
}

.notes-title {
  font-size: 16px;
  font-weight: 700;
  color: var(--text-primary);
}

.notes-subtitle {
  margin-top: 4px;
  font-size: 12px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 220px;
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
  color: var(--text-muted);
  text-align: center;
}

.notes-empty-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
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
  border: 1px solid var(--line-soft);
  border-radius: 10px;
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  cursor: pointer;
  background: rgba(255, 255, 255, 0.02);
  transition: border-color 140ms ease, background 140ms ease;
}

.note-card:hover {
  border-color: rgba(106, 169, 255, 0.3);
}

.note-card.active {
  border-color: rgba(106, 169, 255, 0.5);
  background: rgba(106, 169, 255, 0.08);
}

.note-card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.note-page {
  font-size: 11px;
  font-weight: 600;
  color: var(--accent, #6aa9ff);
  background: rgba(106, 169, 255, 0.12);
  padding: 1px 8px;
  border-radius: 999px;
}

.note-time {
  font-size: 11px;
  color: var(--text-muted);
}

.note-quote {
  font-size: 12px;
  color: var(--text-muted);
  line-height: 1.45;
  border-left: 2px solid var(--line-soft);
  padding-left: 8px;
}

.note-content {
  font-size: 13px;
  color: var(--text-primary);
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
  color: var(--text-muted);
  cursor: pointer;
  font-size: 12px;
  padding: 2px 6px;
  border-radius: 6px;
}

.note-actions button:hover {
  color: var(--text-primary);
  background: rgba(255, 255, 255, 0.05);
}

.note-actions button.danger:hover {
  color: #ffb3b3;
  background: rgba(255, 99, 99, 0.1);
}
</style>
