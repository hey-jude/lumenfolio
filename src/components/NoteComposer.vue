<script setup>
import { computed, nextTick, ref, watch } from 'vue'

const props = defineProps({
  show: {
    type: Boolean,
    default: false,
  },
  mode: {
    type: String,
    default: 'create', // create | edit
  },
  quoteText: {
    type: String,
    default: '',
  },
  initialContent: {
    type: String,
    default: '',
  },
  saving: {
    type: Boolean,
    default: false,
  },
  ui: {
    type: Object,
    required: true,
  },
})

const emit = defineEmits(['save', 'cancel'])

const draft = ref('')
const textareaRef = ref(null)

const quotePreview = computed(() => {
  const text = String(props.quoteText || '').replace(/\s+/g, ' ').trim()
  if (!text) return ''
  return text.length > 220 ? `${text.slice(0, 220)}...` : text
})

watch(
  () => props.show,
  (visible) => {
    if (!visible) return
    draft.value = props.initialContent || ''
    nextTick(() => textareaRef.value?.focus())
  },
  { immediate: true },
)

function handleSave() {
  if (props.saving) return
  // Empty content is allowed (pure highlight) for create mode only.
  emit('save', draft.value.trim())
}
</script>

<template>
  <div v-if="show" class="note-composer-backdrop" @click.self="emit('cancel')">
    <section class="note-composer" role="dialog" aria-modal="true">
      <div class="composer-title">
        {{ mode === 'edit' ? ui.noteEditTitle : ui.noteComposerTitle }}
      </div>
      <div v-if="quotePreview" class="composer-quote">"{{ quotePreview }}"</div>
      <textarea
        ref="textareaRef"
        v-model="draft"
        class="composer-input"
        rows="4"
        :placeholder="ui.noteEditorPlaceholder"
        @keydown.esc.prevent="emit('cancel')"
        @keydown.meta.enter.prevent="handleSave"
        @keydown.ctrl.enter.prevent="handleSave"
      ></textarea>
      <div class="composer-actions">
        <button type="button" class="composer-btn" @click="emit('cancel')">
          {{ ui.noteCancel }}
        </button>
        <button type="button" class="composer-btn primary" :disabled="saving" @click="handleSave">
          {{ ui.noteSave }}
        </button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.note-composer-backdrop {
  position: fixed;
  inset: 0;
  z-index: 1200;
  background: var(--scrim);
  display: flex;
  align-items: center;
  justify-content: center;
}

.note-composer {
  width: min(460px, calc(100vw - 32px));
  border: 1px solid var(--line);
  border-radius: var(--r-lg);
  background: var(--surface-2);
  box-shadow: var(--shadow-overlay);
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.composer-title {
  font-size: 15px;
  font-weight: var(--w-strong);
  color: var(--ink);
}

.composer-quote {
  font-size: 12px;
  color: var(--ink-3);
  line-height: 1.45;
  border-left: 2px solid var(--line);
  padding: 4px 0 4px 10px;
  max-height: 110px;
  overflow: auto;
}

.composer-input {
  width: 100%;
  box-sizing: border-box;
  resize: vertical;
  min-height: 90px;
  border: 1px solid var(--line);
  border-radius: var(--r-md);
  background: var(--surface-wash);
  color: var(--ink);
  font-size: 13px;
  line-height: 1.5;
  padding: 10px 12px;
  outline: none;
}

.composer-input:focus {
  border-color: var(--accent-line);
}

.composer-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.composer-btn {
  min-height: 34px;
  padding: 0 14px;
  border-radius: var(--r-md);
  border: 1px solid var(--line);
  background: var(--surface-wash);
  color: var(--ink-2);
  cursor: pointer;
  font-size: 13px;
}

.composer-btn:hover {
  color: var(--ink);
}

.composer-btn.primary {
  border-color: var(--accent-line);
  background: var(--accent-tint);
  color: var(--ink);
}

.composer-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
