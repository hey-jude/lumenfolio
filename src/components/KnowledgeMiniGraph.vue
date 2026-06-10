<script setup>
import { computed, onBeforeUnmount, ref, watch } from 'vue'

// Reading-time knowledge graph for one document: the open paper in the centre,
// its top concepts/entities around it (size = salience), and related papers on
// an outer ring. A concept shared with a related paper is positioned between
// the two and drawn as a bridge — so "why related" is visible in the picture
// itself. With no related papers yet (cold start) it degrades to a concept
// ring, never to a blank area.
//
// Hand-drawn SVG on purpose — ≤ ~22 nodes total, so a sigma/WebGL instance
// would be pure overhead next to the full KnowledgeGraphView.
const props = defineProps({
  centerTitle: { type: String, default: '' },
  // Card terms: [{ name, detail, salience }] — concepts and entities merged.
  terms: { type: Array, default: () => [] },
  // [{ documentId, title, score, sharedConcepts[], coCitation }]
  related: { type: Array, default: () => [] },
  // Inter-links among the related docs: [{ docA, docB, sharedCount, coCitation }]
  links: { type: Array, default: () => [] },
  // Canvas size (viewBox units) and term budget — the full-pane variant passes
  // a larger canvas and more terms; `fill` makes the svg occupy the container
  // height instead of scaling by width.
  width: { type: Number, default: 560 },
  height: { type: Number, default: 250 },
  maxTerms: { type: Number, default: 10 },
  fill: { type: Boolean, default: false },
  ui: { type: Object, required: true },
})

const emit = defineEmits(['open-doc'])

// Fill mode measures the actual container in pixels (ResizeObserver) and drives
// the viewBox + SVG size from that — NOT from a percentage/calc height. A
// percentage height resolved against a flex-derived ancestor isn't definite on
// the first layout pass (it only "fixes up" after a reflow, e.g. opening
// DevTools), so the graph would render at height 0 until something forced a
// resize. Measured pixels are definite on the observer's initial callback and
// update on every resize.
const HINT_RESERVE = 20 // px kept below the svg for the one-line hint
const containerRef = ref(null)
const measuredW = ref(0)
const measuredH = ref(0)
let resizeObserver = null

const W = computed(() => (props.fill && measuredW.value ? measuredW.value : props.width))
const H = computed(() => (props.fill && measuredH.value ? measuredH.value : props.height))
const CX = computed(() => W.value / 2)
const CY = computed(() => H.value / 2 + 4)
const labelMax = computed(() => (W.value > 700 ? 30 : 20))
const GOLDEN = 2.399963

function measure() {
  const el = containerRef.value
  if (!el) return
  if (el.clientWidth) measuredW.value = el.clientWidth
  if (el.clientHeight) measuredH.value = Math.max(0, el.clientHeight - HINT_RESERVE)
}

// Attach the observer whenever the container element appears — not just at
// mount. The `.mini-graph` root is behind `v-if="!empty"`, so on the first
// frame (terms still loading) containerRef can be null; an onMounted-only hook
// would then never attach. `flush: 'post'` runs after the DOM is patched so
// clientWidth/Height are real, and `immediate` covers the already-present case.
watch(
  containerRef,
  (el) => {
    if (resizeObserver) { resizeObserver.disconnect(); resizeObserver = null }
    if (!props.fill || !el || typeof ResizeObserver === 'undefined') return
    resizeObserver = new ResizeObserver(measure)
    resizeObserver.observe(el)
    measure()
    // A second pass next frame guards against a 0-size first layout pass that
    // the observer's initial callback occasionally races (Tauri/WKWebView).
    requestAnimationFrame(measure)
  },
  { immediate: true, flush: 'post' },
)

onBeforeUnmount(() => {
  if (resizeObserver) resizeObserver.disconnect()
  resizeObserver = null
})

const hovered = ref(null) // { type: 'doc'|'term', node }

function shortLabel(text, max = 20) {
  const t = String(text || '')
    .replace(/\.pdf$/i, '')
    .replace(/^\d{4}\.\d{4,5}(v\d+)?\s*/, '') // strip a leading arXiv id
    .trim()
  return t.length > max ? `${t.slice(0, max - 1)}…` : t
}

const maxScore = computed(() =>
  props.related.reduce((max, item) => Math.max(max, item.score || 0), 0) || 1)

// Related docs on the outer ring, strongest first from 12 o'clock.
const docNodes = computed(() => {
  const items = [...props.related].sort((a, b) => (b.score || 0) - (a.score || 0))
  const n = items.length
  const cx = CX.value
  const cy = CY.value
  const rx = W.value * 0.4
  const ry = H.value * 0.37
  return items.map((item, index) => {
    const angle = -Math.PI / 2 + (index * 2 * Math.PI) / n
    const x = cx + rx * Math.cos(angle)
    const y = cy + ry * Math.sin(angle)
    const t = (item.score || 0) / maxScore.value
    const r = 5 + 5 * t
    return {
      documentId: item.documentId,
      title: item.title,
      sharedConcepts: Array.isArray(item.sharedConcepts) ? item.sharedConcepts : [],
      coCitation: Number(item.coCitation) || 0,
      angle,
      x,
      y,
      r,
      label: shortLabel(item.title, labelMax.value),
      anchor: x < cx - 24 ? 'end' : x > cx + 24 ? 'start' : 'middle',
      labelY: y < cy ? y - r - 6 : y + r + 13,
    }
  })
})

// Term selection: top salience, plus — for every related doc — at least one of
// its shared concepts, so no related doc floats unexplained.
const termNodes = computed(() => {
  const pool = props.terms
    .filter((item) => item && item.name)
    .map((item) => ({
      name: item.name,
      detail: item.detail || '',
      salience: Math.min(Math.max(Number(item.salience) || 0, 0), 1),
    }))
  pool.sort((a, b) => b.salience - a.salience)
  const byKey = new Map(pool.map((item) => [item.name.toLowerCase(), item]))

  const selected = []
  const selectedKeys = new Set()
  const push = (item) => {
    const key = item.name.toLowerCase()
    if (selectedKeys.has(key)) return
    selectedKeys.add(key)
    selected.push(item)
  }
  for (const item of pool.slice(0, props.maxTerms)) push(item)
  for (const doc of docNodes.value) {
    const sharedKeys = doc.sharedConcepts.map((name) => name.toLowerCase())
    if (sharedKeys.some((key) => selectedKeys.has(key))) continue
    const first = doc.sharedConcepts[0]
    if (!first) continue // co-citation-only relation, bridged by a direct edge
    push(byKey.get(first.toLowerCase()) || { name: first, detail: '', salience: 0.45 })
  }

  // Which docs each term bridges to (shared-concept name match).
  const docsByTermKey = new Map()
  for (const doc of docNodes.value) {
    for (const name of doc.sharedConcepts) {
      const key = name.toLowerCase()
      if (!docsByTermKey.has(key)) docsByTermKey.set(key, [])
      docsByTermKey.get(key).push(doc)
    }
  }

  const cx = CX.value
  const cy = CY.value
  const rx = W.value * 0.23
  const ry = H.value * 0.22
  const placed = selected.map((item, index) => {
    const docs = docsByTermKey.get(item.name.toLowerCase()) || []
    let angle
    if (docs.length) {
      // Circular mean of the bridged docs' angles, with a small per-index
      // jitter so terms bridging the same doc don't stack.
      const sx = docs.reduce((sum, d) => sum + Math.cos(d.angle), 0)
      const sy = docs.reduce((sum, d) => sum + Math.sin(d.angle), 0)
      angle = Math.atan2(sy, sx) + (index % 3 - 1) * 0.35
    } else {
      angle = -Math.PI / 2 + 0.6 + index * GOLDEN
    }
    const x = cx + rx * Math.cos(angle)
    const y = cy + ry * Math.sin(angle)
    const r = 3.5 + 4.5 * item.salience
    return {
      name: item.name,
      detail: item.detail,
      docs,
      x,
      y,
      r,
      label: shortLabel(item.name, labelMax.value - 2),
      anchor: x < cx - 16 ? 'end' : x > cx + 16 ? 'start' : 'middle',
      labelY: y < cy ? y - r - 4 : y + r + 11,
    }
  })
  return placed
})

const docById = computed(() => {
  const map = new Map()
  for (const node of docNodes.value) map.set(node.documentId, node)
  return map
})

const interEdges = computed(() => props.links
  .map((link) => {
    const a = docById.value.get(link.docA)
    const b = docById.value.get(link.docB)
    if (!a || !b) return null
    return { a, b }
  })
  .filter(Boolean))

const bridgeEdges = computed(() => termNodes.value.flatMap((term) =>
  term.docs.map((doc) => ({ term, doc }))))

const empty = computed(() => !termNodes.value.length && !docNodes.value.length)

function isDimmed(node, type) {
  const h = hovered.value
  if (!h) return false
  if (h.type === type && h.node === node) return false
  if (h.type === 'doc' && type === 'term') return !node.docs.includes(h.node)
  if (h.type === 'term' && type === 'doc') return !h.node.docs.includes(node)
  return true
}

const hint = computed(() => {
  const h = hovered.value
  if (!h) return props.ui.knowledgeMiniGraphHint
  if (h.type === 'term') {
    const parts = [h.node.name]
    if (h.node.detail) parts.push(h.node.detail)
    return parts.join(' — ')
  }
  const parts = []
  if (h.node.sharedConcepts.length) {
    parts.push(`${props.ui.graphEdgeShared}: ${h.node.sharedConcepts.join(' · ')}`)
  }
  if (h.node.coCitation > 0) {
    parts.push(`${props.ui.knowledgeRelatedCoCited} ×${Math.round(h.node.coCitation)}`)
  }
  return parts.join(' — ') || h.node.title
})
</script>

<template>
  <div v-if="!empty" ref="containerRef" class="mini-graph" :class="{ fill }">
    <svg
      :viewBox="`0 0 ${W} ${H}`"
      preserveAspectRatio="xMidYMid meet"
      :style="fill ? { position: 'absolute', top: '0', left: '0', width: `${W}px`, height: `${H}px` } : null"
    >
      <line
        v-for="(edge, index) in interEdges"
        :key="`i-${index}`"
        class="mini-inter"
        :x1="edge.a.x" :y1="edge.a.y" :x2="edge.b.x" :y2="edge.b.y"
      />
      <line
        v-for="term in termNodes"
        :key="`ct-${term.name}`"
        class="mini-stem"
        :x1="CX" :y1="CY" :x2="term.x" :y2="term.y"
      />
      <line
        v-for="(edge, index) in bridgeEdges"
        :key="`b-${index}`"
        class="mini-bridge"
        :class="{ dim: isDimmed(edge.doc, 'doc') && isDimmed(edge.term, 'term') }"
        :x1="edge.term.x" :y1="edge.term.y" :x2="edge.doc.x" :y2="edge.doc.y"
      />
      <line
        v-for="doc in docNodes.filter((d) => d.coCitation > 0)"
        :key="`cc-${doc.documentId}`"
        class="mini-cocite"
        :class="{ dim: isDimmed(doc, 'doc') }"
        :x1="CX" :y1="CY" :x2="doc.x" :y2="doc.y"
      />

      <g
        v-for="term in termNodes"
        :key="`t-${term.name}`"
        class="mini-term"
        :class="{ dim: isDimmed(term, 'term') }"
        @mouseenter="hovered = { type: 'term', node: term }"
        @mouseleave="hovered = null"
      >
        <title>{{ term.detail ? `${term.name} — ${term.detail}` : term.name }}</title>
        <circle :cx="term.x" :cy="term.y" :r="term.r" />
        <text :x="term.x" :y="term.labelY" :text-anchor="term.anchor">{{ term.label }}</text>
      </g>

      <g
        v-for="doc in docNodes"
        :key="doc.documentId"
        class="mini-doc"
        :class="{ dim: isDimmed(doc, 'doc') }"
        @mouseenter="hovered = { type: 'doc', node: doc }"
        @mouseleave="hovered = null"
        @click="emit('open-doc', doc.documentId)"
      >
        <title>{{ doc.title }}</title>
        <circle :cx="doc.x" :cy="doc.y" :r="doc.r" />
        <text :x="doc.x" :y="doc.labelY" :text-anchor="doc.anchor">{{ doc.label }}</text>
      </g>

      <g class="mini-center">
        <title>{{ centerTitle }}</title>
        <circle :cx="CX" :cy="CY" r="12" />
      </g>
    </svg>
    <p class="mini-hint" :class="{ active: hovered }">{{ hint }}</p>
  </div>
</template>

<style scoped>
.mini-graph {
  width: 100%;
}

.mini-graph svg {
  display: block;
  width: 100%;
  height: auto;
}

/* Fill variant: the SVG's pixel size comes from a ResizeObserver measuring this
   container (see script), bound as inline width/height — NOT from any
   percentage/calc height. A %/flex height isn't definite on the first layout
   pass, so the graph stayed at height 0 until a reflow (e.g. opening DevTools)
   forced it. The svg is absolutely positioned so its explicit pixel height
   can't feed back into the container's flex-derived height. */
.mini-graph.fill {
  flex: 1 1 0%;
  min-height: 0;
  position: relative;
}

.mini-graph.fill .mini-hint {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  margin: 0;
}

.mini-stem {
  stroke: rgba(255, 255, 255, 0.12);
  stroke-width: 1;
}

.mini-bridge {
  stroke: rgba(192, 132, 252, 0.5);
  stroke-width: 1.4;
  transition: opacity 120ms ease;
}

.mini-cocite {
  stroke: rgba(192, 132, 252, 0.75);
  stroke-width: 2;
  transition: opacity 120ms ease;
}

.mini-inter {
  stroke: rgba(255, 255, 255, 0.13);
  stroke-width: 1;
  stroke-dasharray: 3 3;
}

.mini-bridge.dim,
.mini-cocite.dim {
  opacity: 0.2;
}

.mini-term circle {
  fill: #c084fc;
  fill-opacity: 0.75;
  stroke: rgba(255, 255, 255, 0.2);
  stroke-width: 0.8;
}

.mini-term text {
  font-size: 9.5px;
  fill: var(--text-muted);
  pointer-events: none;
}

.mini-term:hover circle {
  fill-opacity: 1;
}

.mini-term:hover text {
  fill: var(--text-secondary);
}

.mini-term.dim,
.mini-doc.dim {
  opacity: 0.3;
}

.mini-term,
.mini-doc {
  transition: opacity 120ms ease;
}

.mini-doc {
  cursor: pointer;
}

.mini-doc circle {
  fill: #6aa9ff;
  fill-opacity: 0.85;
  stroke: rgba(255, 255, 255, 0.25);
  stroke-width: 1;
}

.mini-doc:hover circle {
  fill-opacity: 1;
}

.mini-doc text {
  font-size: 10.5px;
  fill: var(--text-secondary);
  pointer-events: none;
}

.mini-doc:hover text {
  fill: var(--text-primary);
}

.mini-center circle {
  fill: #6aa9ff;
  stroke: rgba(255, 255, 255, 0.6);
  stroke-width: 1.5;
}

.mini-hint {
  margin: 2px 0 0;
  min-height: 16px;
  font-size: 10.5px;
  line-height: 1.45;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mini-hint.active {
  color: var(--text-secondary);
}
</style>
