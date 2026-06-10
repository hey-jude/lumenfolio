<script setup>
import { computed, ref } from 'vue'

const props = defineProps({
  // { status, summary, entities[], concepts[], keywords[], error }
  card: { type: Object, default: null },
  // Live status override from the in-flight precipitation queue ('running' | undefined).
  liveStatus: { type: String, default: '' },
  // Related papers: [{ documentId, title, sharedConcepts[], coCitation }].
  related: { type: Array, default: () => [] },
  ui: { type: Object, required: true },
})

const emit = defineEmits(['reprecipitate', 'open-doc'])

const collapsed = ref(false)

const status = computed(() => props.liveStatus || props.card?.status || 'idle')
const summary = computed(() => props.card?.summary || '')
const entities = computed(() => (Array.isArray(props.card?.entities) ? props.card.entities : []))
const concepts = computed(() => (Array.isArray(props.card?.concepts) ? props.card.concepts : []))
const keywords = computed(() => (Array.isArray(props.card?.keywords) ? props.card.keywords : []))

// Hide entirely when there's nothing to show and nothing happening.
const visible = computed(() => (
  status.value === 'running'
  || status.value === 'failed'
  || Boolean(summary.value)
  || entities.value.length > 0
  || concepts.value.length > 0
  || props.related.length > 0
))

const statusLabel = computed(() => {
  if (status.value === 'running' || status.value === 'pending') return props.ui.knowledgeRunning
  if (status.value === 'failed') return props.ui.knowledgeFailed
  return ''
})
</script>

<template>
  <div v-if="visible" class="knowledge-card" :class="{ collapsed }">
    <div class="knowledge-head">
      <button type="button" class="knowledge-toggle" @click="collapsed = !collapsed">
        <span class="knowledge-caret" :class="{ open: !collapsed }">▸</span>
        <span class="knowledge-title">🧠 {{ ui.knowledge }}</span>
      </button>
      <span v-if="statusLabel" class="knowledge-status" :class="status">{{ statusLabel }}</span>
      <span class="knowledge-spacer"></span>
      <button
        type="button"
        class="knowledge-reprecipitate"
        :disabled="status === 'running'"
        :title="ui.knowledgeReprecipitate"
        @click="emit('reprecipitate')"
      >↻</button>
    </div>

    <div v-if="!collapsed" class="knowledge-body">
      <p v-if="summary" class="knowledge-summary">{{ summary }}</p>
      <div v-if="status === 'failed' && card?.error" class="knowledge-error">{{ card.error }}</div>

      <div v-if="entities.length" class="knowledge-group">
        <span class="knowledge-group-label">{{ ui.knowledgeEntities }}</span>
        <span
          v-for="(item, index) in entities"
          :key="`e-${index}`"
          class="knowledge-chip entity"
          :title="item.detail || item.name"
        >{{ item.name }}</span>
      </div>

      <div v-if="concepts.length" class="knowledge-group">
        <span class="knowledge-group-label">{{ ui.knowledgeConcepts }}</span>
        <span
          v-for="(item, index) in concepts"
          :key="`c-${index}`"
          class="knowledge-chip concept"
          :title="item.detail || item.name"
        >{{ item.name }}</span>
      </div>

      <div v-if="keywords.length" class="knowledge-group">
        <span class="knowledge-group-label">{{ ui.knowledgeKeywords }}</span>
        <span class="knowledge-keywords">{{ keywords.join(' · ') }}</span>
      </div>

      <div v-if="related.length" class="knowledge-related">
        <div class="knowledge-group-label">{{ ui.knowledgeRelated }}</div>
        <button
          v-for="item in related"
          :key="item.documentId"
          type="button"
          class="knowledge-related-item"
          @click="emit('open-doc', item.documentId)"
        >
          <span class="knowledge-related-title">{{ item.title }}</span>
          <span v-if="item.coCitation > 0" class="knowledge-related-badge" :title="ui.knowledgeRelatedCoCited">↔</span>
          <span v-if="item.sharedConcepts?.length" class="knowledge-related-reason">
            {{ item.sharedConcepts.join(' · ') }}
          </span>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.knowledge-card {
  flex-shrink: 0;
  border-bottom: 1px solid var(--line-soft);
  background: var(--bg-panel);
  max-height: 38vh;
  overflow-y: auto;
}

.knowledge-head {
  position: sticky;
  top: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 14px;
  background: var(--bg-panel);
}

.knowledge-toggle {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: none;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  padding: 0;
  font-size: 13px;
  font-weight: 600;
}

.knowledge-caret {
  display: inline-block;
  transition: transform 140ms ease;
  color: var(--text-muted);
  font-size: 11px;
}

.knowledge-caret.open {
  transform: rotate(90deg);
}

.knowledge-status {
  font-size: 11px;
  padding: 1px 8px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-secondary);
}

.knowledge-status.running,
.knowledge-status.pending {
  color: #f0b54a;
  background: rgba(240, 181, 74, 0.14);
}

.knowledge-status.failed {
  color: #ffb3b3;
  background: rgba(198, 73, 73, 0.14);
}

.knowledge-spacer {
  flex: 1;
}

.knowledge-reprecipitate {
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border: 1px solid var(--line-soft);
  border-radius: 7px;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 14px;
}

.knowledge-reprecipitate:hover:not(:disabled) {
  color: var(--text-primary);
  border-color: rgba(106, 169, 255, 0.4);
}

.knowledge-reprecipitate:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.knowledge-body {
  padding: 0 14px 12px;
}

.knowledge-summary {
  margin: 0 0 10px;
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--text-secondary);
}

.knowledge-error {
  margin-bottom: 10px;
  font-size: 12px;
  color: #ffb3b3;
}

.knowledge-group {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin-bottom: 8px;
}

.knowledge-group-label {
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-muted);
  margin-right: 2px;
}

.knowledge-chip {
  font-size: 11px;
  padding: 2px 9px;
  border-radius: 999px;
  border: 1px solid var(--line-soft);
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-secondary);
  cursor: default;
}

.knowledge-chip.entity {
  border-color: rgba(106, 169, 255, 0.35);
  color: #cfe0ff;
}

.knowledge-chip.concept {
  border-color: rgba(192, 132, 252, 0.35);
  color: #e6d6ff;
}

.knowledge-keywords {
  font-size: 11.5px;
  color: var(--text-muted);
  line-height: 1.5;
}

.knowledge-related {
  margin-top: 4px;
  padding-top: 8px;
  border-top: 1px solid var(--line-soft);
}

.knowledge-related .knowledge-group-label {
  display: block;
  margin-bottom: 6px;
}

.knowledge-related-item {
  display: flex;
  align-items: baseline;
  gap: 8px;
  width: 100%;
  padding: 5px 8px;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  text-align: left;
}

.knowledge-related-item:hover {
  background: rgba(255, 255, 255, 0.04);
  border-color: rgba(106, 169, 255, 0.25);
}

.knowledge-related-title {
  flex-shrink: 0;
  max-width: 45%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--text-primary);
}

.knowledge-related-badge {
  flex-shrink: 0;
  color: var(--accent);
  font-size: 11px;
}

.knowledge-related-reason {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
  color: var(--text-muted);
}
</style>
