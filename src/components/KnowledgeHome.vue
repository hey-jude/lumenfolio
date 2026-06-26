<script setup>
import { ref, computed } from 'vue'

// Knowledge-base pivot (P1): the landing surface when no document is open. The
// home centers "ask my knowledge base" (a library-wide question), with quick
// entry points into recent sources and precipitated concepts.
const props = defineProps({
  ui: { type: Object, required: true },
  // Recent sources: [{ id, shortTitle, title, contentType }]
  recentDocs: { type: Array, default: () => [] },
  // Precipitated concept labels for quick filtering/exploration.
  concepts: { type: Array, default: () => [] },
  // Whether a chat model is configured (gates the ask box).
  modelConfigured: { type: Boolean, default: true },
})

const emit = defineEmits(['ask', 'open-doc'])

const question = ref('')

const topConcepts = computed(() => props.concepts.slice(0, 8))
const recent = computed(() => props.recentDocs.slice(0, 8))

function submitAsk() {
  const text = question.value.trim()
  if (!text || !props.modelConfigured) return
  emit('ask', text)
  question.value = ''
}

function askConcept(label) {
  emit('ask', `${props.ui.aboutConceptPrefix || ''}${label}`.trim() || label)
}

function docKindLabel(doc) {
  const map = props.ui.contentTypeLabels || {}
  return map[doc.contentType] || (doc.contentType ? doc.contentType.toUpperCase() : 'PDF')
}
</script>

<template>
  <div class="kb-home">
    <div class="kb-home-inner">
      <h1 class="kb-title">{{ ui.myKnowledgeBase }}</h1>

      <form class="kb-ask" @submit.prevent="submitAsk">
        <input
          v-model="question"
          class="kb-ask-input"
          type="text"
          :disabled="!modelConfigured"
          :placeholder="modelConfigured ? ui.askKnowledgeBasePlaceholder : ui.modelNotConfiguredHint"
          @keydown.enter.prevent="submitAsk"
        />
        <button class="kb-ask-send" type="submit" :disabled="!modelConfigured || !question.trim()" :aria-label="ui.sendMessage">
          <span class="kb-send-icon" aria-hidden="true"></span>
        </button>
      </form>

      <div v-if="topConcepts.length" class="kb-section">
        <div class="kb-section-title">{{ ui.concepts }}</div>
        <div class="kb-chips">
          <button
            v-for="label in topConcepts"
            :key="label"
            type="button"
            class="kb-chip"
            :disabled="!modelConfigured"
            @click="askConcept(label)"
          >{{ label }}</button>
        </div>
      </div>

      <div v-if="recent.length" class="kb-section">
        <div class="kb-section-title">{{ ui.recent }}</div>
        <ul class="kb-recent">
          <li v-for="doc in recent" :key="doc.id">
            <button type="button" class="kb-recent-item" @click="emit('open-doc', doc.id)">
              <span class="kb-recent-name">{{ doc.shortTitle || doc.title || doc.id }}</span>
              <span class="kb-recent-kind">{{ docKindLabel(doc) }}</span>
            </button>
          </li>
        </ul>
      </div>

      <p v-if="!recent.length && !topConcepts.length" class="kb-empty">
        {{ ui.knowledgeHomeEmpty }}
      </p>
    </div>
  </div>
</template>

<style scoped>
.kb-home {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  justify-content: center;
  background: var(--bg-base, var(--bg-panel));
}

.kb-home-inner {
  width: 100%;
  max-width: 720px;
  padding: 64px 28px 40px;
}

.kb-title {
  font-size: 22px;
  font-weight: 500;
  color: var(--text-primary);
  margin: 0 0 20px;
}

.kb-ask {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid var(--line-soft);
  border-radius: 12px;
  background: var(--bg-elevated, rgba(255, 255, 255, 0.04));
  padding: 6px 6px 6px 16px;
}

.kb-ask-input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: transparent;
  color: var(--text-primary);
  font-size: 15px;
  height: 40px;
}

.kb-ask-send {
  width: 36px;
  height: 36px;
  flex: 0 0 auto;
  border-radius: 9px;
  border: none;
  background: var(--accent, #6aa9ff);
  color: #fff;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.kb-ask-send:disabled {
  opacity: 0.45;
  cursor: default;
}

.kb-send-icon {
  width: 14px;
  height: 14px;
  position: relative;
  display: inline-block;
}

.kb-send-icon::before {
  content: '';
  position: absolute;
  left: 1px;
  top: 6px;
  width: 11px;
  height: 2px;
  background: currentColor;
}

.kb-send-icon::after {
  content: '';
  position: absolute;
  right: 1px;
  top: 2px;
  width: 7px;
  height: 7px;
  border-top: 2px solid currentColor;
  border-right: 2px solid currentColor;
  transform: rotate(45deg);
}

.kb-section {
  margin-top: 28px;
}

.kb-section-title {
  font-size: 12px;
  color: var(--text-muted, var(--text-secondary));
  margin-bottom: 10px;
}

.kb-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.kb-chip {
  border: 1px solid var(--line-soft);
  background: var(--accent-soft, rgba(106, 169, 255, 0.12));
  color: var(--accent, #6aa9ff);
  border-radius: 999px;
  padding: 5px 12px;
  font-size: 13px;
  cursor: pointer;
}

.kb-chip:disabled {
  opacity: 0.5;
  cursor: default;
}

.kb-recent {
  list-style: none;
  margin: 0;
  padding: 0;
}

.kb-recent-item {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  background: transparent;
  border: none;
  border-radius: 8px;
  padding: 9px 12px;
  cursor: pointer;
  color: var(--text-primary);
  font-size: 14px;
  text-align: left;
}

.kb-recent-item:hover {
  background: var(--bg-elevated, rgba(255, 255, 255, 0.05));
}

.kb-recent-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.kb-recent-kind {
  flex: 0 0 auto;
  font-size: 11px;
  color: var(--text-muted, var(--text-secondary));
}

.kb-empty {
  margin-top: 28px;
  color: var(--text-muted, var(--text-secondary));
  font-size: 14px;
}
</style>
