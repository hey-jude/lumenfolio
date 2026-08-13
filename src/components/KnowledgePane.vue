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
  background: var(--surface-1);
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
  color: var(--ink-3);
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
  border-left: 1px solid var(--line);
}

.kp-side-head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px 8px;
  border-bottom: 1px solid var(--line);
}

.kp-status {
  font-size: 11px;
  padding: 1px 8px;
  border-radius: var(--r-pill);
  background: var(--surface-hover);
  color: var(--ink-2);
}

.kp-status.running,
.kp-status.pending {
  color: var(--warning);
  background: var(--warning-tint);
}

.kp-status.failed {
  color: var(--danger);
  background: var(--danger-tint);
}

.kp-spacer {
  flex: 1;
}

.kp-link {
  border: none;
  background: transparent;
  color: var(--ink-3);
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
  border: 1px solid var(--line);
  border-radius: var(--r-sm);
  background: transparent;
  color: var(--ink-3);
  cursor: pointer;
  font-size: 14px;
}

.kp-reprecipitate:hover:not(:disabled) {
  color: var(--ink);
  border-color: var(--accent-line);
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
  color: var(--danger);
}

.kp-summary {
  margin: 0 0 14px;
  font-size: 12.5px;
  line-height: 1.6;
  color: var(--ink-2);
}

.kp-group {
  margin-bottom: 14px;
}

.kp-group-label {
  font-size: 10px;
  font-weight: var(--w-medium);
  letter-spacing: var(--tracking-caps);
  text-transform: uppercase;
  color: var(--ink-3);
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
  border-radius: var(--r-pill);
  border: 1px solid var(--line);
  background: var(--surface-wash);
  color: var(--ink-2);
  cursor: default;
}

.kp-chip.entity {
  border-color: var(--accent-line);
  color: var(--accent-ink);
}

.kp-chip.concept {
  border-color: var(--accent-line);
  color: #e6d6ff;
}

.kp-keywords {
  font-size: 11.5px;
  color: var(--ink-3);
  line-height: 1.5;
}

.kp-related {
  padding-top: 10px;
  border-top: 1px solid var(--line);
}

.kp-related-item {
  display: block;
  width: 100%;
  padding: 6px 8px;
  border: 1px solid transparent;
  border-radius: var(--r-md);
  background: transparent;
  color: var(--ink-2);
  cursor: pointer;
  text-align: left;
}

.kp-related-item:hover {
  background: var(--surface-wash);
  border-color: var(--accent-line);
}

.kp-related-title {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--ink);
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
  color: var(--ink-3);
}
</style>
