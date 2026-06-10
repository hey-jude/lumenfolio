<script setup>
import { computed } from 'vue'
import KnowledgeMiniGraph from './KnowledgeMiniGraph.vue'

// Full-reader-area knowledge view (toggled by the toolbar 阅读/知识 tabs).
// Left ~2/3: the concept-bridge graph at full size. Right ~1/3: scrollable
// text details (summary, entity/concept chips, keywords, related list).
// Rendered as an overlay above the PDF viewer (v-show) so switching tabs
// never resets the reading position.
const props = defineProps({
  // { status, summary, entities[], concepts[], keywords[], error }
  card: { type: Object, default: null },
  // Live status override from the in-flight precipitation queue ('running' | undefined).
  liveStatus: { type: String, default: '' },
  // Related papers: [{ documentId, title, score, sharedConcepts[], coCitation }].
  related: { type: Array, default: () => [] },
  // Inter-links among the related docs: [{ docA, docB, sharedCount, coCitation }].
  relatedLinks: { type: Array, default: () => [] },
  documentTitle: { type: String, default: '' },
  ui: { type: Object, required: true },
})

const emit = defineEmits(['reprecipitate', 'open-doc', 'open-graph'])

const status = computed(() => props.liveStatus || props.card?.status || 'idle')
const summary = computed(() => props.card?.summary || '')
const entities = computed(() => (Array.isArray(props.card?.entities) ? props.card.entities : []))
const concepts = computed(() => (Array.isArray(props.card?.concepts) ? props.card.concepts : []))
const keywords = computed(() => (Array.isArray(props.card?.keywords) ? props.card.keywords : []))

const graphTerms = computed(() => [...concepts.value, ...entities.value])
const hasGraph = computed(() => graphTerms.value.length > 0 || props.related.length > 0)
const hasText = computed(() => (
  Boolean(summary.value) || entities.value.length > 0 || concepts.value.length > 0
))

const statusLabel = computed(() => {
  if (status.value === 'running' || status.value === 'pending') return props.ui.knowledgeRunning
  if (status.value === 'failed') return props.ui.knowledgeFailed
  return ''
})
</script>

<template>
  <div class="knowledge-pane">
    <div class="kp-graph">
      <KnowledgeMiniGraph
        v-if="hasGraph"
        :center-title="documentTitle"
        :terms="graphTerms"
        :related="related"
        :links="relatedLinks"
        :width="760"
        :height="700"
        :max-terms="14"
        :fill="true"
        :ui="ui"
        @open-doc="(id) => emit('open-doc', id)"
      />
      <div v-else class="kp-placeholder">
        <template v-if="status === 'running' || status === 'pending'">{{ ui.knowledgeRunning }}</template>
        <template v-else-if="status === 'failed'">{{ ui.knowledgeFailed }}</template>
        <template v-else>{{ ui.knowledgeEmpty }}</template>
      </div>
    </div>

    <aside class="kp-side">
      <div class="kp-side-head">
        <span v-if="statusLabel" class="kp-status" :class="status">{{ statusLabel }}</span>
        <span class="kp-spacer"></span>
        <button type="button" class="kp-link" @click="emit('open-graph')">
          {{ ui.knowledgeViewInGraph }} ↗
        </button>
        <button
          type="button"
          class="kp-reprecipitate"
          :disabled="status === 'running'"
          :title="ui.knowledgeReprecipitate"
          @click="emit('reprecipitate')"
        >↻</button>
      </div>

      <div class="kp-side-body">
        <div v-if="status === 'failed' && card?.error" class="kp-error">{{ card.error }}</div>

        <p v-if="summary" class="kp-summary">{{ summary }}</p>

        <div v-if="entities.length" class="kp-group">
          <div class="kp-group-label">{{ ui.knowledgeEntities }}</div>
          <div class="kp-chips">
            <span
              v-for="(item, index) in entities"
              :key="`e-${index}`"
              class="kp-chip entity"
              :title="item.detail || item.name"
            >{{ item.name }}</span>
          </div>
        </div>

        <div v-if="concepts.length" class="kp-group">
          <div class="kp-group-label">{{ ui.knowledgeConcepts }}</div>
          <div class="kp-chips">
            <span
              v-for="(item, index) in concepts"
              :key="`c-${index}`"
              class="kp-chip concept"
              :title="item.detail || item.name"
            >{{ item.name }}</span>
          </div>
        </div>

        <div v-if="keywords.length" class="kp-group">
          <div class="kp-group-label">{{ ui.knowledgeKeywords }}</div>
          <div class="kp-keywords">{{ keywords.join(' · ') }}</div>
        </div>

        <div v-if="related.length" class="kp-group kp-related">
          <div class="kp-group-label">{{ ui.knowledgeRelated }} ({{ related.length }})</div>
          <button
            v-for="item in related"
            :key="item.documentId"
            type="button"
            class="kp-related-item"
            @click="emit('open-doc', item.documentId)"
          >
            <span class="kp-related-title" :title="item.title">{{ item.title }}</span>
            <span class="kp-related-meta">
              <span v-if="item.coCitation > 0" class="kp-related-badge" :title="ui.knowledgeRelatedCoCited">↔</span>
              <span v-if="item.sharedConcepts?.length" class="kp-related-reason">
                {{ item.sharedConcepts.join(' · ') }}
              </span>
            </span>
          </button>
        </div>

        <div v-if="!hasText && !related.length && status !== 'failed'" class="kp-placeholder small">
          {{ ui.knowledgeEmpty }}
        </div>
      </div>
    </aside>
  </div>
</template>

<style scoped>
.knowledge-pane {
  position: absolute;
  inset: 0;
  z-index: 5;
  display: flex;
  min-height: 0;
  background: var(--bg-panel);
}

.kp-graph {
  flex: 1 1 64%;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 10px 4px 8px 14px;
}

.kp-placeholder {
  flex: 1;
  display: grid;
  place-items: center;
  color: var(--text-muted);
  font-size: 13px;
  text-align: center;
  padding: 20px;
}

.kp-placeholder.small {
  flex: none;
  padding: 24px 8px;
}

.kp-side {
  flex: 0 0 34%;
  max-width: 380px;
  min-width: 260px;
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-left: 1px solid var(--line-soft);
}

.kp-side-head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px 8px;
  border-bottom: 1px solid var(--line-soft);
}

.kp-status {
  font-size: 11px;
  padding: 1px 8px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-secondary);
}

.kp-status.running,
.kp-status.pending {
  color: #f0b54a;
  background: rgba(240, 181, 74, 0.14);
}

.kp-status.failed {
  color: #ffb3b3;
  background: rgba(198, 73, 73, 0.14);
}

.kp-spacer {
  flex: 1;
}

.kp-link {
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 11.5px;
  padding: 0;
}

.kp-link:hover {
  color: var(--accent);
}

.kp-reprecipitate {
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

.kp-reprecipitate:hover:not(:disabled) {
  color: var(--text-primary);
  border-color: rgba(106, 169, 255, 0.4);
}

.kp-reprecipitate:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.kp-side-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 12px 14px;
}

.kp-error {
  margin-bottom: 10px;
  font-size: 12px;
  color: #ffb3b3;
}

.kp-summary {
  margin: 0 0 14px;
  font-size: 12.5px;
  line-height: 1.6;
  color: var(--text-secondary);
}

.kp-group {
  margin-bottom: 14px;
}

.kp-group-label {
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-muted);
  margin-bottom: 6px;
}

.kp-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.kp-chip {
  font-size: 11px;
  padding: 2px 9px;
  border-radius: 999px;
  border: 1px solid var(--line-soft);
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-secondary);
  cursor: default;
}

.kp-chip.entity {
  border-color: rgba(106, 169, 255, 0.35);
  color: #cfe0ff;
}

.kp-chip.concept {
  border-color: rgba(192, 132, 252, 0.35);
  color: #e6d6ff;
}

.kp-keywords {
  font-size: 11.5px;
  color: var(--text-muted);
  line-height: 1.5;
}

.kp-related {
  padding-top: 10px;
  border-top: 1px solid var(--line-soft);
}

.kp-related-item {
  display: block;
  width: 100%;
  padding: 6px 8px;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  text-align: left;
}

.kp-related-item:hover {
  background: rgba(255, 255, 255, 0.04);
  border-color: rgba(106, 169, 255, 0.25);
}

.kp-related-title {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--text-primary);
}

.kp-related-meta {
  display: flex;
  align-items: baseline;
  gap: 6px;
  margin-top: 1px;
}

.kp-related-badge {
  flex-shrink: 0;
  color: var(--accent);
  font-size: 11px;
}

.kp-related-reason {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
  color: var(--text-muted);
}
</style>
