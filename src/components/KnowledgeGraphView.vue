<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import Graph from 'graphology'
import Sigma from 'sigma'
import forceAtlas2 from 'graphology-layout-forceatlas2'
import louvain from 'graphology-communities-louvain'

const props = defineProps({
  // { documents:[{id,title}], artifacts:[{documentId,kind,name,normalized,salience}], links:[{docA,docB,basis,weight}] }
  data: { type: Object, default: null },
  status: { type: String, default: 'idle' }, // idle | loading | loaded | failed
  error: { type: String, default: '' },
  selectedDocId: { type: String, default: '' },
  // Start in focus (ego) mode around selectedDocId — used by the knowledge
  // card's "view in graph" shortcut. Read once at mount (the view remounts on
  // every open, so a reactive watch would be redundant).
  initialFocus: { type: Boolean, default: false },
  ui: { type: Object, required: true },
})

const emit = defineEmits(['open-doc', 'refresh', 'close'])

const containerRef = ref(null)
const mode = ref('concept') // 'concept' | 'document'
const colorBy = ref('type') // 'type' | 'community'
const focusMode = ref(props.initialFocus && Boolean(props.selectedDocId)) // ego-graph around the open document
const collapseCommunities = ref(false) // render communities as super-nodes
const expandedCommunities = ref(new Set()) // communities shown in detail
const showInsights = ref(false)
const insights = ref({ surprising: [], gaps: [], bridges: [] })
const insightsCount = computed(() =>
  insights.value.surprising.length + insights.value.gaps.length + insights.value.bridges.length)

function toggleCollapse() {
  collapseCommunities.value = !collapseCommunities.value
  // Reset which communities are expanded each time the mode is toggled.
  expandedCommunities.value = new Set()
}

// Locate a node by label substring: highlight it (ego) and centre the camera.
function locateNode() {
  const query = searchQuery.value.trim().toLowerCase()
  if (!query || !sigma || !graph.value) {
    if (hoveredNode) {
      hoveredNode = null
      sigma?.refresh()
    }
    return
  }
  let match = null
  graph.value.forEachNode((node, attrs) => {
    if (match) return
    if (String(attrs.label || '').toLowerCase().includes(query)) match = node
  })
  if (!match) return
  hoveredNode = match
  const pos = sigma.getNodeDisplayData(match)
  if (pos) {
    sigma.getCamera().animate({ x: pos.x, y: pos.y, ratio: 0.4 }, { duration: 400 })
  }
  sigma.refresh()
}

// Pin the highlight to a node (used by insight clicks); open documents.
function focusGraphNode(nodeId, isDocument) {
  if (isDocument) {
    emit('open-doc', nodeId)
    return
  }
  if (sigma && graph.value && graph.value.hasNode(nodeId)) {
    hoveredNode = nodeId
    sigma.refresh()
  }
}

let sigma = null
const graph = shallowRef(null)
let hoveredNode = null
const hasNodes = ref(false)
const edgeTip = ref('') // click-an-edge explanation ("shared: …" / "co-cited")
const searchQuery = ref('')

const NODE_COLORS = {
  document: '#6aa9ff',
  concept: '#c084fc',
  entity: '#60a5fa',
}
const COMMUNITY_COLORS = [
  '#60a5fa', '#4ade80', '#fb923c', '#c084fc', '#f87171',
  '#2dd4bf', '#facc15', '#f472b6', '#a78bfa', '#34d399',
]
// Concepts shared by more documents than this are too generic to connect on in
// document mode (would create a hairball + O(k^2) edges) — skip them.
const GENERIC_CONCEPT_DOC_LIMIT = 10

// Keep at most this many strongest edges per node (anti-hairball pruning).
const TOP_K_EDGES = 6

function conceptKey(normalized) {
  return `c:${normalized}`
}

// Structural insights computed on the full graph: surprising cross-community
// links, isolated documents (knowledge gaps), and bridge documents.
function analyzeInsights(g) {
  const out = { surprising: [], gaps: [], bridges: [] }
  if (g.order === 0) {
    insights.value = out
    return
  }
  let comm = {}
  try {
    comm = louvain(g, { resolution: 1 })
  } catch {
    g.forEachNode((node) => { comm[node] = 0 })
  }

  g.forEachNode((node, attrs) => {
    if (attrs.kind !== 'document') return
    if (g.degree(node) === 0) {
      out.gaps.push({ id: node, label: attrs.label })
      return
    }
    const comms = new Set()
    g.forEachNeighbor(node, (n) => comms.add(comm[n]))
    if (comms.size >= 3) out.bridges.push({ id: node, label: attrs.label, count: comms.size })
  })

  const crossEdges = []
  g.forEachEdge((edge, attrs, s, t) => {
    if (comm[s] !== comm[t]) {
      crossEdges.push({ s, t, w: attrs.weight || 0 })
    }
  })
  crossEdges.sort((a, b) => b.w - a.w)
  out.surprising = crossEdges.slice(0, 5).map((e) => ({
    aId: e.s,
    bId: e.t,
    a: g.getNodeAttribute(e.s, 'label'),
    b: g.getNodeAttribute(e.t, 'label'),
  }))

  out.gaps = out.gaps.slice(0, 8)
  out.bridges.sort((a, b) => b.count - a.count)
  out.bridges = out.bridges.slice(0, 8)
  insights.value = out
}

// Reduce the graph for legibility: optional ego-focus (1–2 hops around the open
// document) + top-k strongest edges per node, then drop nodes left isolated.
function pruneAndFocus(g) {
  // Ego-focus: keep only the focus document's 2-hop neighbourhood.
  if (focusMode.value && props.selectedDocId && g.hasNode(props.selectedDocId)) {
    const keep = new Set([props.selectedDocId])
    g.forEachNeighbor(props.selectedDocId, (n) => keep.add(n))
    for (const n of [...keep]) g.forEachNeighbor(n, (m) => keep.add(m))
    for (const node of g.nodes()) {
      if (!keep.has(node)) g.dropNode(node)
    }
  }

  // Top-k edge pruning: keep each node's strongest edges, drop the rest.
  if (g.size > 0) {
    const keepEdges = new Set()
    g.forEachNode((node) => {
      const edges = g.edges(node).map((e) => ({ e, w: g.getEdgeAttribute(e, 'weight') || 0 }))
      edges.sort((a, b) => b.w - a.w)
      edges.slice(0, TOP_K_EDGES).forEach(({ e }) => keepEdges.add(e))
    })
    const drop = []
    g.forEachEdge((edge) => {
      if (!keepEdges.has(edge)) drop.push(edge)
    })
    drop.forEach((edge) => g.dropEdge(edge))
  }

  // Drop now-isolated nodes (e.g. a concept whose only edges were pruned).
  const isolated = []
  g.forEachNode((node) => {
    if (g.degree(node) === 0) isolated.push(node)
  })
  isolated.forEach((node) => {
    // Always keep the focus document even if isolated.
    if (node !== props.selectedDocId) g.dropNode(node)
  })
}

function buildGraph() {
  const data = props.data
  const g = new Graph({ multi: false, type: 'undirected' })
  if (!data) return g
  const documents = Array.isArray(data.documents) ? data.documents : []
  const artifacts = Array.isArray(data.artifacts) ? data.artifacts : []
  const links = Array.isArray(data.links) ? data.links : []

  for (const doc of documents) {
    if (!g.hasNode(doc.id)) {
      g.addNode(doc.id, { kind: 'document', label: doc.title || doc.id })
    }
  }

  // Representative name per normalized concept/entity.
  const conceptInfo = new Map()
  for (const a of artifacts) {
    if (!a.normalized || !g.hasNode(a.documentId)) continue
    if (!conceptInfo.has(a.normalized)) {
      conceptInfo.set(a.normalized, { name: a.name || a.normalized, kind: a.kind || 'concept' })
    }
  }

  if (mode.value === 'concept') {
    for (const [norm, info] of conceptInfo) {
      g.addNode(conceptKey(norm), { kind: info.kind, label: info.name })
    }
    for (const a of artifacts) {
      const cid = conceptKey(a.normalized)
      if (a.normalized && g.hasNode(a.documentId) && g.hasNode(cid) && !g.hasEdge(a.documentId, cid)) {
        g.addEdge(a.documentId, cid, {
          weight: Math.max(0.3, Number(a.salience) || 0.5),
          relation: 'mentions',
        })
      }
    }
    for (const l of links) {
      if (l.basis !== 'co_citation') continue
      if (g.hasNode(l.docA) && g.hasNode(l.docB) && !g.hasEdge(l.docA, l.docB)) {
        g.addEdge(l.docA, l.docB, { weight: Number(l.weight) || 1, relation: 'co_citation' })
      }
    }
  } else {
    // Document mode: collapse concepts into aggregated doc<->doc edges, weighted
    // by the 4-signal relevance (Adamic-Adar: rarer shared concept weighs more;
    // entity affinity; co_citation as the strong direct link).
    const docsByConcept = new Map() // normalized -> Set(docId)
    for (const a of artifacts) {
      if (!a.normalized || !g.hasNode(a.documentId)) continue
      if (!docsByConcept.has(a.normalized)) docsByConcept.set(a.normalized, new Set())
      docsByConcept.get(a.normalized).add(a.documentId)
    }
    const addDocEdge = (x, y, w, relation, sharedName) => {
      if (x === y || !g.hasNode(x) || !g.hasNode(y)) return
      if (g.hasEdge(x, y)) {
        g.updateEdgeAttribute(x, y, 'weight', (prev) => (prev || 0) + w)
        if (relation === 'co_citation') g.setEdgeAttribute(x, y, 'relation', 'co_citation')
        if (sharedName) {
          g.updateEdgeAttribute(x, y, 'sharedConcepts', (prev) => {
            const list = prev || []
            if (list.length < 12 && !list.includes(sharedName)) list.push(sharedName)
            return list
          })
        }
      } else {
        g.addEdge(x, y, { weight: w, relation, sharedConcepts: sharedName ? [sharedName] : [] })
      }
    }
    for (const [norm, set] of docsByConcept) {
      if (set.size < 2 || set.size > GENERIC_CONCEPT_DOC_LIMIT) continue
      // Adamic-Adar: 1/ln(docFreq); entity affinity bump.
      const aa = 1 / Math.log(Math.max(set.size, 2))
      const affinity = conceptInfo.get(norm)?.kind === 'entity' ? 1.3 : 1.0
      const w = (1 + aa * 2) * affinity // overlap baseline + Adamic-Adar term
      const name = conceptInfo.get(norm)?.name || norm
      const arr = [...set]
      for (let i = 0; i < arr.length; i++) {
        for (let j = i + 1; j < arr.length; j++) addDocEdge(arr[i], arr[j], w, 'shared_concept', name)
      }
    }
    for (const l of links) {
      if (l.basis !== 'co_citation') continue
      addDocEdge(l.docA, l.docB, (Number(l.weight) || 1) * 3, 'co_citation')
    }
  }

  // Insights are computed on the FULL graph (before pruning/focus drops nodes).
  analyzeInsights(g)
  if (collapseCommunities.value) {
    return collapseGraph(g)
  }
  pruneAndFocus(g)
  applyVisualsAndLayout(g, { runLouvain: true })
  return g
}

// Louvain communities + node visuals (size/color/seed position) + forceatlas2.
function applyVisualsAndLayout(g, { runLouvain = true } = {}) {
  if (runLouvain && g.order > 0 && g.size > 0) {
    try {
      louvain.assign(g, { resolution: 1 })
    } catch {
      g.forEachNode((node) => g.setNodeAttribute(node, 'community', 0))
    }
  }
  let maxDegree = 1
  g.forEachNode((node) => {
    maxDegree = Math.max(maxDegree, g.degree(node))
  })
  // Distinct per-node seed angle: using node-id string length collapses many nodes
  // onto the same start position (forceatlas2 can't separate coincident points).
  let seed = 0
  g.forEachNode((node, attrs) => {
    const degree = g.degree(node)
    const kind = attrs.kind
    const base = kind === 'community' ? 6 : kind === 'document' ? 4 : 2.5
    const growth = kind === 'document' ? 9 : kind === 'community' ? 4 : 5
    const memberBoost = kind === 'community' ? Math.sqrt(attrs.memberCount || 1) * 2 : 0
    if (g.getNodeAttribute(node, 'community') == null) g.setNodeAttribute(node, 'community', 0)
    const angle = seed * 2.399963 // golden-angle increment → well-spread seeds
    seed += 1
    g.setNodeAttribute(node, 'x', Math.cos(angle) * (1 + (degree % 5) * 0.12))
    g.setNodeAttribute(node, 'y', Math.sin(angle) * (1 + (degree % 5) * 0.12))
    g.setNodeAttribute(node, 'size', base + memberBoost + Math.sqrt(degree / maxDegree) * growth)
    g.setNodeAttribute(node, 'baseColor', colorForNode(attrs, g.getNodeAttribute(node, 'community')))
    g.setNodeAttribute(node, 'color', g.getNodeAttribute(node, 'baseColor'))
  })
  if (g.order > 0) {
    try {
      const settings = forceAtlas2.inferSettings(g)
      const iterations = g.order > 1200 ? 60 : g.order > 400 ? 120 : 260
      forceAtlas2.assign(g, { iterations, settings })
    } catch {
      // Positions already seeded above — layout failure is non-fatal.
    }
  }
}

// Collapse Louvain communities into super-nodes; expanded communities show their
// real members. Clicking a super-node toggles its expansion.
function collapseGraph(base) {
  if (base.order > 0 && base.size > 0) {
    try {
      louvain.assign(base, { resolution: 1 })
    } catch {
      base.forEachNode((node) => base.setNodeAttribute(node, 'community', 0))
    }
  }
  const communityOf = {}
  const memberCount = {}
  base.forEachNode((node) => {
    const c = base.getNodeAttribute(node, 'community') ?? 0
    communityOf[node] = c
    memberCount[c] = (memberCount[c] || 0) + 1
  })

  const g = new Graph({ multi: false, type: 'undirected' })
  // Map each base node to its display unit id (member node or community super-node).
  const unitOf = (node) => {
    const c = communityOf[node]
    return expandedCommunities.value.has(c) ? node : `comm:${c}`
  }
  base.forEachNode((node, attrs) => {
    const c = communityOf[node]
    if (expandedCommunities.value.has(c)) {
      if (!g.hasNode(node)) {
        g.addNode(node, { kind: attrs.kind, label: attrs.label, community: c })
      }
    } else {
      const id = `comm:${c}`
      if (!g.hasNode(id)) {
        g.addNode(id, {
          kind: 'community',
          label: `${props.ui.graphCommunityLabel} ${c} (${memberCount[c]})`,
          community: c,
          communityId: c,
          memberCount: memberCount[c],
        })
      }
    }
  })
  base.forEachEdge((edge, attrs, s, t) => {
    const us = unitOf(s)
    const ut = unitOf(t)
    if (us === ut || !g.hasNode(us) || !g.hasNode(ut)) return
    const w = attrs.weight || 1
    if (g.hasEdge(us, ut)) {
      g.updateEdgeAttribute(us, ut, 'weight', (prev) => (prev || 0) + w)
    } else {
      g.addEdge(us, ut, { weight: w, relation: 'community' })
    }
  })
  applyVisualsAndLayout(g, { runLouvain: false })
  return g
}

// Recolour nodes in place without rebuilding/relaying out (used by the Type↔
// Community toggle, which only changes colour, not topology or position).
function recolor() {
  const g = graph.value
  if (!sigma || !g) {
    if (props.data) render()
    return
  }
  g.forEachNode((node, attrs) => {
    const color = colorForNode(attrs, g.getNodeAttribute(node, 'community'))
    g.setNodeAttribute(node, 'baseColor', color)
    g.setNodeAttribute(node, 'color', color)
  })
  sigma.refresh()
}

function colorForNode(attrs, community) {
  if (colorBy.value === 'community') {
    const index = Number.isFinite(community) ? community : 0
    return COMMUNITY_COLORS[((index % COMMUNITY_COLORS.length) + COMMUNITY_COLORS.length) % COMMUNITY_COLORS.length]
  }
  return NODE_COLORS[attrs.kind] || '#94a3b8'
}

async function render() {
  if (!containerRef.value) return
  const g = buildGraph()
  graph.value = g
  hasNodes.value = g.order > 0
  if (g.order === 0) {
    if (sigma) { sigma.kill(); sigma = null }
    return
  }
  // The canvas is `display:none` while !hasNodes; we just set hasNodes=true, but
  // Vue applies that class change on the next tick. Wait for it so the container
  // is laid out (has width) before Sigma reads its size — otherwise Sigma throws
  // "Container has no width". allowInvalidContainer is a belt-and-suspenders for
  // the case where the whole graph view is itself still 0-size at first paint.
  await nextTick()
  if (!containerRef.value) return
  // Kill AFTER the await (not before) so two render() calls racing through the
  // tick can't both create a Sigma and leak the first.
  if (sigma) { sigma.kill(); sigma = null }
  sigma = new Sigma(g, containerRef.value, {
    allowInvalidContainer: true,
    renderLabels: true,
    labelDensity: 0.6,
    labelRenderedSizeThreshold: 7,
    defaultEdgeColor: 'rgba(120,130,150,0.25)',
    nodeReducer: (node, attrs) => {
      const res = { ...attrs }
      if (hoveredNode && node !== hoveredNode && !g.areNeighbors(node, hoveredNode)) {
        res.color = 'rgba(120,130,150,0.12)'
        res.label = ''
      }
      return res
    },
    edgeReducer: (edge, attrs) => {
      const res = { ...attrs }
      const weight = g.getEdgeAttribute(edge, 'weight') || 1
      res.size = Math.min(5, 0.6 + Math.sqrt(weight) * 0.9)
      if (g.getEdgeAttribute(edge, 'relation') === 'co_citation') {
        res.color = 'rgba(106,169,255,0.5)'
      }
      if (hoveredNode && !g.hasExtremity(edge, hoveredNode)) {
        res.color = 'rgba(120,130,150,0.05)'
      }
      return res
    },
  })

  sigma.on('clickNode', ({ node }) => {
    const kind = g.getNodeAttribute(node, 'kind')
    if (kind === 'community') {
      // Expand this community into its member documents.
      const cid = g.getNodeAttribute(node, 'communityId')
      const next = new Set(expandedCommunities.value)
      next.add(cid)
      expandedCommunities.value = next
      return
    }
    if (kind === 'document') {
      emit('open-doc', node)
    } else {
      // Concept node: pin hover-highlight to it (show the docs that mention it).
      hoveredNode = hoveredNode === node ? null : node
      sigma.refresh()
    }
  })
  sigma.on('enterNode', ({ node }) => {
    hoveredNode = node
    sigma.refresh()
  })
  sigma.on('leaveNode', () => {
    hoveredNode = null
    sigma.refresh()
  })
  sigma.on('clickEdge', ({ edge }) => {
    const relation = g.getEdgeAttribute(edge, 'relation')
    const shared = g.getEdgeAttribute(edge, 'sharedConcepts') || []
    if (relation === 'co_citation' && !shared.length) {
      edgeTip.value = props.ui.graphEdgeCoCited
    } else if (shared.length) {
      edgeTip.value = `${props.ui.graphEdgeShared}: ${shared.join(' · ')}`
    } else {
      const [s, t] = g.extremities(edge)
      const sc = g.getNodeAttribute(s, 'kind') === 'concept' || g.getNodeAttribute(s, 'kind') === 'entity'
      edgeTip.value = g.getNodeAttribute(sc ? s : t, 'label') || ''
    }
  })
  sigma.on('clickStage', () => {
    edgeTip.value = ''
    if (hoveredNode) {
      hoveredNode = null
      sigma.refresh()
    }
  })
}

onMounted(() => {
  if (props.data) render()
})

onBeforeUnmount(() => {
  if (sigma) {
    sigma.kill()
    sigma = null
  }
})

watch(() => props.data, (data) => {
  if (data) render()
})
// Structural changes rebuild the graph + re-layout; recolouring is cheap.
watch([mode, focusMode, collapseCommunities, expandedCommunities], () => {
  if (props.data) render()
})
watch(colorBy, () => {
  if (props.data) recolor()
})
</script>

<template>
  <div class="graph-view">
    <div class="graph-head" data-tauri-drag-region>
      <div class="graph-title">🕸 {{ ui.knowledgeGraph }}</div>
      <input
        v-model="searchQuery"
        type="text"
        class="graph-search"
        :placeholder="ui.graphSearch"
        @input="locateNode"
        @keyup.enter="locateNode"
      />
      <div class="graph-controls">
        <div class="graph-segmented">
          <button :class="{ active: mode === 'concept' }" @click="mode = 'concept'">{{ ui.graphModeConcept }}</button>
          <button :class="{ active: mode === 'document' }" @click="mode = 'document'">{{ ui.graphModeDocument }}</button>
        </div>
        <div class="graph-segmented">
          <button :class="{ active: colorBy === 'type' }" @click="colorBy = 'type'">{{ ui.graphColorType }}</button>
          <button :class="{ active: colorBy === 'community' }" @click="colorBy = 'community'">{{ ui.graphColorCommunity }}</button>
        </div>
        <button
          v-if="selectedDocId"
          class="graph-refresh"
          :class="{ 'graph-toggle-on': focusMode }"
          :title="ui.graphFocusHint"
          @click="focusMode = !focusMode"
        >{{ ui.graphFocus }}</button>
        <button
          class="graph-refresh"
          :class="{ 'graph-toggle-on': collapseCommunities }"
          :title="ui.graphCollapseHint"
          @click="toggleCollapse"
        >{{ ui.graphCollapse }}</button>
        <button
          class="graph-refresh"
          :class="{ 'graph-toggle-on': showInsights }"
          @click="showInsights = !showInsights"
        >{{ ui.graphInsights }} {{ insightsCount }}</button>
        <button class="graph-refresh" :disabled="status === 'loading'" @click="emit('refresh')">{{ ui.refresh }}</button>
        <button class="graph-close" :title="ui.graphClose" :aria-label="ui.graphClose" @click="emit('close')">✕</button>
      </div>
    </div>

    <div class="graph-legend">
      <span><i class="dot" style="background:#6aa9ff"></i>{{ ui.graphLegendDocument }}</span>
      <span><i class="dot" style="background:#c084fc"></i>{{ ui.graphLegendConcept }}</span>
      <span><i class="dot" style="background:#60a5fa"></i>{{ ui.graphLegendEntity }}</span>
    </div>

    <div class="graph-stage">
      <div v-if="status === 'loading'" class="graph-state">{{ ui.graphLoading }}</div>
      <div v-else-if="status === 'failed'" class="graph-state graph-state-error">
        <div>{{ error || ui.graphFailed }}</div>
        <button class="graph-retry" @click="emit('refresh')">{{ ui.retry }}</button>
      </div>
      <div v-else-if="status === 'loaded' && !hasNodes" class="graph-state">{{ ui.graphEmpty }}</div>
      <div ref="containerRef" class="graph-canvas" :class="{ hidden: status !== 'loaded' || !hasNodes }"></div>

      <div v-if="showInsights && status === 'loaded' && hasNodes" class="graph-insights">
        <div class="insights-section" v-if="insights.surprising.length">
          <div class="insights-label">{{ ui.graphInsightSurprising }}</div>
          <button
            v-for="(item, index) in insights.surprising"
            :key="`s-${index}`"
            class="insights-item"
            @click="focusGraphNode(item.aId, false)"
          >{{ item.a }} <span class="insights-link">↔</span> {{ item.b }}</button>
        </div>
        <div class="insights-section" v-if="insights.bridges.length">
          <div class="insights-label">{{ ui.graphInsightBridges }}</div>
          <button
            v-for="item in insights.bridges"
            :key="`b-${item.id}`"
            class="insights-item"
            @click="focusGraphNode(item.id, true)"
          >{{ item.label }} <span class="insights-meta">×{{ item.count }}</span></button>
        </div>
        <div class="insights-section" v-if="insights.gaps.length">
          <div class="insights-label">{{ ui.graphInsightGaps }}</div>
          <button
            v-for="item in insights.gaps"
            :key="`g-${item.id}`"
            class="insights-item"
            @click="focusGraphNode(item.id, true)"
          >{{ item.label }}</button>
        </div>
        <div v-if="!insightsCount" class="insights-empty">{{ ui.graphInsightEmpty }}</div>
      </div>

      <div v-if="edgeTip" class="graph-edge-tip" @click="edgeTip = ''">{{ edgeTip }}</div>
    </div>
  </div>
</template>

<style scoped>
.graph-view {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--bg-app);
}

.graph-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 0 18px;
  min-height: 60px;
  border-bottom: 1px solid var(--line-soft);
  flex-wrap: wrap;
}

.graph-title {
  font-size: 15px;
  font-weight: 700;
  color: var(--text-primary);
}

.graph-search {
  flex: 0 1 200px;
  min-width: 120px;
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-soft);
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-primary);
  font-size: 12px;
  outline: none;
}

.graph-controls {
  display: flex;
  align-items: center;
  gap: 10px;
}

.graph-segmented {
  display: inline-flex;
  border: 1px solid var(--line-soft);
  border-radius: 999px;
  overflow: hidden;
}

.graph-segmented button {
  border: none;
  background: transparent;
  color: var(--text-secondary);
  padding: 5px 12px;
  font-size: 12px;
  cursor: pointer;
}

.graph-segmented button.active {
  background: rgba(106, 169, 255, 0.16);
  color: var(--text-primary);
}

.graph-refresh,
.graph-retry {
  border: 1px solid var(--line-soft);
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-secondary);
  padding: 5px 12px;
  font-size: 12px;
  cursor: pointer;
}

.graph-toggle-on {
  border-color: rgba(106, 169, 255, 0.5) !important;
  background: rgba(106, 169, 255, 0.16) !important;
  color: var(--text-primary) !important;
}

.graph-close {
  width: 28px;
  height: 28px;
  display: grid;
  place-items: center;
  border: 1px solid var(--line-soft);
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 13px;
  margin-left: 2px;
}

.graph-close:hover {
  color: var(--text-primary);
  border-color: rgba(255, 120, 120, 0.5);
  background: rgba(255, 120, 120, 0.12);
}

.graph-legend {
  display: flex;
  gap: 16px;
  padding: 7px 18px;
  font-size: 11px;
  color: var(--text-muted);
  border-bottom: 1px solid var(--line-soft);
}

.graph-legend .dot {
  display: inline-block;
  width: 9px;
  height: 9px;
  border-radius: 999px;
  margin-right: 5px;
  vertical-align: middle;
}

.graph-stage {
  position: relative;
  flex: 1;
  min-height: 0;
}

.graph-canvas {
  position: absolute;
  inset: 0;
}

.graph-canvas.hidden {
  display: none;
}

.graph-state {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  color: var(--text-muted);
  font-size: 13px;
}

.graph-state-error {
  color: #ffb3b3;
}

.graph-insights {
  position: absolute;
  top: 12px;
  right: 12px;
  width: 280px;
  max-height: calc(100% - 24px);
  overflow-y: auto;
  padding: 12px 14px;
  border: 1px solid var(--line-soft);
  border-radius: 12px;
  background: rgba(29, 31, 35, 0.96);
  backdrop-filter: blur(8px);
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
}

.insights-section {
  margin-bottom: 12px;
}

.insights-label {
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-muted);
  margin-bottom: 6px;
}

.insights-item {
  display: block;
  width: 100%;
  text-align: left;
  padding: 5px 8px;
  margin-bottom: 3px;
  border: 1px solid transparent;
  border-radius: 7px;
  background: transparent;
  color: var(--text-secondary);
  font-size: 12px;
  cursor: pointer;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.insights-item:hover {
  background: rgba(255, 255, 255, 0.05);
  border-color: rgba(106, 169, 255, 0.25);
  color: var(--text-primary);
}

.insights-link {
  color: var(--accent);
}

.insights-meta {
  color: var(--text-muted);
  font-size: 11px;
}

.insights-empty {
  font-size: 12px;
  color: var(--text-muted);
}

.graph-edge-tip {
  position: absolute;
  left: 50%;
  bottom: 16px;
  transform: translateX(-50%);
  max-width: 70%;
  padding: 6px 14px;
  border: 1px solid rgba(106, 169, 255, 0.4);
  border-radius: 999px;
  background: rgba(29, 31, 35, 0.96);
  color: var(--text-primary);
  font-size: 12px;
  cursor: pointer;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
