<script setup>
/**
 * A pane, card, or floating layer — the thing every panel in this app was
 * hand-rolling with its own background hex and its own box-shadow.
 *
 * `level` is elevation order, not a color: 0 is the page floor, 3 floats above
 * everything. Pick the level for where the surface sits in the stack and the
 * palette follows.
 *
 * `elevation` is separate on purpose. A dense in-flow card wants `hairline`
 * (just the ring); only genuinely floating chrome — menus, modals — should
 * reach for `overlay`, and no more than two of those should be on screen at
 * once. On this dark canvas the blur layers barely register; the 1px ring in
 * the first shadow slot is what actually draws the edge.
 */
const props = defineProps({
  /** 0 = page, 1 = pane, 2 = card, 3 = floating. */
  level: { type: [Number, String], default: 1 },
  /** Recessed instead of raised: code wells, read-only regions. */
  inset: { type: Boolean, default: false },
  /** none | hairline | card | raised | overlay */
  elevation: { type: String, default: 'hairline' },
  /** none | xs | sm | md | lg | pill */
  radius: { type: String, default: 'md' },
  as: { type: String, default: 'div' },
})
</script>

<template>
  <component
    :is="props.as"
    class="ui-surface"
    :class="[
      props.inset ? 'lvl-inset' : `lvl-${props.level}`,
      `el-${props.elevation}`,
      `r-${props.radius}`,
    ]"
  >
    <slot />
  </component>
</template>

<style scoped>
.ui-surface {
  color: var(--ink);
}

.lvl-0 {
  background: var(--surface-0);
}
.lvl-1 {
  background: var(--surface-1);
}
.lvl-2 {
  background: var(--surface-2);
}
.lvl-3 {
  background: var(--surface-3);
}
.lvl-inset {
  background: var(--surface-inset);
}

.el-none {
  box-shadow: none;
}
.el-hairline {
  box-shadow: var(--shadow-hairline);
}
.el-card {
  box-shadow: var(--shadow-card);
}
.el-raised {
  box-shadow: var(--shadow-raised);
}
.el-overlay {
  box-shadow: var(--shadow-overlay);
}

.r-none {
  border-radius: 0;
}
.r-xs {
  border-radius: var(--r-xs);
}
.r-sm {
  border-radius: var(--r-sm);
}
.r-md {
  border-radius: var(--r-md);
}
.r-lg {
  border-radius: var(--r-lg);
}
.r-pill {
  border-radius: var(--r-pill);
}
</style>
