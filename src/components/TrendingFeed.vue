<script setup>
const props = defineProps({
  papers: { type: Array, default: () => [] },
  status: { type: String, default: 'idle' }, // idle | loading | loaded | failed
  error: { type: String, default: '' },
  addError: { type: String, default: '' },
  addingIds: { type: Array, default: () => [] },
  ui: { type: Object, required: true },
  locale: { type: String, default: 'en' },
})

const emit = defineEmits(['refresh', 'add-paper', 'open-hf'])

function authorLine(paper) {
  const authors = Array.isArray(paper.authors) ? paper.authors : []
  if (!authors.length) return ''
  const shown = authors.slice(0, 4).join(', ')
  return authors.length > 4 ? `${shown} et al.` : shown
}

function publishedDate(paper) {
  return String(paper.publishedAt || '').slice(0, 10)
}

function isAdding(paper) {
  return props.addingIds.includes(paper.arxivId)
}
</script>

<template>
  <div class="trending">
    <div class="trending-head" data-tauri-drag-region>
      <div class="trending-title">{{ ui.trendingPapers }}</div>
      <button
        type="button"
        class="trending-refresh"
        :disabled="status === 'loading'"
        :title="ui.refresh"
        @mousedown.stop
        @click="emit('refresh')"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M20 11a8 8 0 1 0-.6 4" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" />
          <path d="M20 4v6h-6" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
        <span>{{ ui.refresh }}</span>
      </button>
    </div>

    <div class="trending-body">
      <div v-if="addError" class="trending-add-error">{{ addError }}</div>

      <div v-if="status === 'loading'" class="trending-state">{{ ui.trendingLoading }}</div>

      <div v-else-if="status === 'failed'" class="trending-state trending-state-error">
        <div>{{ ui.trendingOffline }}</div>
        <div v-if="error" class="trending-state-detail">{{ error }}</div>
        <button type="button" class="trending-retry" @click="emit('refresh')">{{ ui.retry }}</button>
      </div>

      <div v-else-if="status === 'loaded' && !papers.length" class="trending-state">{{ ui.trendingEmpty }}</div>

      <div v-else class="trending-list">
        <article v-for="paper in papers" :key="paper.arxivId" class="trending-card">
          <img
            v-if="paper.thumbnailUrl"
            :src="paper.thumbnailUrl"
            class="trending-thumb"
            alt=""
            loading="lazy"
          />
          <div class="trending-card-main">
            <div class="trending-card-title">{{ paper.title }}</div>
            <div v-if="authorLine(paper)" class="trending-card-authors">{{ authorLine(paper) }}</div>
            <div class="trending-card-meta">
              <span class="trending-upvotes" :title="ui.upvotes">▲ {{ paper.upvotes }}</span>
              <span v-if="publishedDate(paper)" class="trending-date">{{ publishedDate(paper) }}</span>
              <span class="trending-arxiv">arXiv:{{ paper.arxivId }}</span>
            </div>
            <p v-if="paper.summary" class="trending-card-abstract">{{ paper.summary }}</p>
            <div class="trending-card-actions">
              <button
                type="button"
                class="trending-add"
                :disabled="isAdding(paper)"
                @click="emit('add-paper', paper)"
              >
                {{ isAdding(paper) ? ui.trendingAdding : ui.trendingAdd }}
              </button>
              <button
                type="button"
                class="trending-open-hf"
                @click="emit('open-hf', paper.hfUrl)"
              >
                {{ ui.trendingOpenHf }}
              </button>
            </div>
          </div>
        </article>
      </div>
    </div>
  </div>
</template>

<style scoped>
.trending {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--bg-app);
}

.trending-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 18px;
  min-height: 72px;
  border-bottom: 1px solid var(--line-soft);
}

.trending-title {
  font-size: 16px;
  font-weight: 700;
  color: var(--text-primary);
}

.trending-refresh {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-soft);
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-secondary);
  font-size: 12px;
  cursor: pointer;
}

.trending-refresh svg {
  width: 14px;
  height: 14px;
}

.trending-refresh:hover:not(:disabled) {
  border-color: rgba(106, 169, 255, 0.4);
  color: var(--text-primary);
}

.trending-refresh:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.trending-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 16px 18px 28px;
}

.trending-add-error {
  max-width: 860px;
  margin: 0 auto 12px;
  padding: 8px 12px;
  border: 1px solid rgba(198, 73, 73, 0.4);
  border-radius: 8px;
  background: rgba(198, 73, 73, 0.12);
  color: #ffb3b3;
  font-size: 12px;
}

.trending-state {
  color: var(--text-muted);
  font-size: 13px;
  padding: 32px 0;
  text-align: center;
}

.trending-state-error {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
}

.trending-state-detail {
  font-size: 11px;
  color: var(--text-muted);
  max-width: 420px;
}

.trending-retry,
.trending-add,
.trending-open-hf {
  cursor: pointer;
  border-radius: 8px;
  font-size: 12px;
}

.trending-retry {
  padding: 6px 14px;
  border: 1px solid rgba(106, 169, 255, 0.4);
  background: rgba(106, 169, 255, 0.14);
  color: var(--text-primary);
}

.trending-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 860px;
  margin: 0 auto;
}

.trending-card {
  display: flex;
  gap: 14px;
  padding: 14px;
  border: 1px solid var(--line-soft);
  border-radius: 12px;
  background: var(--bg-panel);
}

.trending-thumb {
  width: 116px;
  height: 76px;
  object-fit: cover;
  border-radius: 8px;
  flex-shrink: 0;
  background: var(--bg-elevated);
}

.trending-card-main {
  min-width: 0;
  flex: 1;
}

.trending-card-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  line-height: 1.35;
}

.trending-card-authors {
  margin-top: 3px;
  font-size: 12px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.trending-card-meta {
  margin-top: 6px;
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  font-size: 11px;
  color: var(--text-muted);
}

.trending-upvotes {
  color: var(--accent);
  font-weight: 600;
}

.trending-card-abstract {
  margin: 8px 0 0;
  font-size: 12.5px;
  line-height: 1.5;
  color: var(--text-secondary);
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.trending-card-actions {
  margin-top: 10px;
  display: flex;
  gap: 8px;
}

.trending-add {
  padding: 5px 14px;
  border: 1px solid rgba(106, 169, 255, 0.45);
  background: rgba(106, 169, 255, 0.16);
  color: var(--text-primary);
  font-weight: 600;
}

.trending-add:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.trending-open-hf {
  padding: 5px 12px;
  border: 1px solid var(--line-soft);
  background: transparent;
  color: var(--text-secondary);
}

.trending-open-hf:hover {
  border-color: rgba(106, 169, 255, 0.4);
  color: var(--text-primary);
}
</style>
