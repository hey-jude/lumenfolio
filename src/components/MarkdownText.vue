<script setup>
import { computed } from 'vue'
import MarkdownIt from 'markdown-it'
import linkAttributes from 'markdown-it-link-attributes'

const props = defineProps({
  text: {
    type: String,
    default: '',
  },
  loading: {
    type: Boolean,
    default: false,
  },
})

const md = new MarkdownIt({
  html: false,
  linkify: true,
  breaks: true,
  highlight(code, language) {
    const lang = language ? ` data-language="${escapeHtml(language)}"` : ''
    return `<pre class="md-code-block"${lang}><code>${escapeHtml(code)}</code></pre>`
  },
})

md.use(linkAttributes, {
  attrs: {
    target: '_blank',
    rel: 'noopener noreferrer',
  },
})

const renderedMarkdown = computed(() => md.render(normalizeMathDelimiters(props.text || '')))
const renderedWithStreamBubble = computed(() => (
  `${renderedMarkdown.value}<span class="stream-bubble" aria-hidden="true"></span>`
))

function normalizeMathDelimiters(value) {
  return String(value || '')
    .replace(/\\\\\(/g, '\\(')
    .replace(/\\\\\)/g, '\\)')
    .replace(/\\\\\[/g, '\\[')
    .replace(/\\\\\]/g, '\\]')
}

function escapeHtml(value) {
  return String(value || '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}
</script>

<template>
  <div class="markdown-text" v-html="loading ? renderedWithStreamBubble : renderedMarkdown"></div>
</template>

<style scoped>
.markdown-text {
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.58;
  white-space: normal;
  overflow-wrap: anywhere;
}

.markdown-text :deep(> :first-child) {
  margin-top: 0;
}

.markdown-text :deep(> :last-child) {
  margin-bottom: 0;
}

.markdown-text :deep(p),
.markdown-text :deep(ul),
.markdown-text :deep(ol),
.markdown-text :deep(blockquote),
.markdown-text :deep(pre),
.markdown-text :deep(table) {
  margin: 0 0 8px;
}

.markdown-text :deep(h1),
.markdown-text :deep(h2),
.markdown-text :deep(h3),
.markdown-text :deep(h4),
.markdown-text :deep(h5),
.markdown-text :deep(h6) {
  margin: 12px 0 7px;
  color: var(--text-primary);
  font-weight: 700;
  line-height: 1.28;
}

.markdown-text :deep(h1) {
  font-size: 17px;
}

.markdown-text :deep(h2) {
  font-size: 15px;
}

.markdown-text :deep(h3),
.markdown-text :deep(h4),
.markdown-text :deep(h5),
.markdown-text :deep(h6) {
  font-size: 13px;
}

.markdown-text :deep(ul),
.markdown-text :deep(ol) {
  padding-left: 18px;
}

.markdown-text :deep(li + li) {
  margin-top: 3px;
}

.markdown-text :deep(li > p) {
  margin: 0;
}

.markdown-text :deep(a) {
  color: var(--accent-text);
  text-decoration: none;
  border-bottom: 1px dashed rgba(106, 169, 255, 0.45);
}

.markdown-text :deep(blockquote) {
  padding-left: 12px;
  border-left: 3px solid rgba(106, 169, 255, 0.35);
  color: var(--text-secondary);
}

.markdown-text :deep(code) {
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 5px;
  padding: 1px 5px;
  background: rgba(255, 255, 255, 0.045);
  color: var(--text-primary);
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 12px;
}

.markdown-text :deep(.md-code-block) {
  overflow: auto;
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 8px;
  padding: 11px 12px;
  background: rgba(0, 0, 0, 0.22);
}

.markdown-text :deep(.md-code-block code) {
  border: 0;
  padding: 0;
  background: transparent;
}

.markdown-text :deep(table) {
  display: block;
  width: 100%;
  overflow: auto;
  border-collapse: collapse;
}

.markdown-text :deep(th),
.markdown-text :deep(td) {
  border: 1px solid rgba(255, 255, 255, 0.08);
  padding: 6px 8px;
}

.markdown-text :deep(.stream-bubble) {
  display: inline-block;
  width: 7px;
  height: 7px;
  margin-left: 6px;
  border-radius: 999px;
  vertical-align: middle;
  background: rgba(106, 169, 255, 0.9);
  animation: stream-bubble-pulse 1.2s ease-in-out infinite;
}

@keyframes stream-bubble-pulse {
  0%,
  100% {
    opacity: 0.35;
    transform: scale(0.85);
  }

  50% {
    opacity: 1;
    transform: scale(1);
  }
}
</style>
