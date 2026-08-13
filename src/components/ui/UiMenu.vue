<script setup>
import { onBeforeUnmount, ref, watch } from 'vue'

/**
 * A floating menu placed at viewport coordinates: the add menu, the row
 * context menu, the session history dropdown.
 *
 * Teleported to <body> because the sidebar establishes its own stacking
 * context — a menu rendered in place gets clipped by, or painted under, the
 * pane it belongs to no matter how high its z-index goes.
 *
 * Position is passed in as a point (usually the pointer, or the anchor's
 * bottom-left) and flipped back inside the viewport here, so callers never
 * have to think about the edge case where the menu opens near the bottom.
 */
const props = defineProps({
  open: { type: Boolean, default: false },
  x: { type: Number, default: 0 },
  y: { type: Number, default: 0 },
  /** Estimated width used for the flip; the menu may render narrower. */
  width: { type: Number, default: 200 },
})

const emit = defineEmits(['close'])

const menuRef = ref(null)
const pos = ref({ left: 0, top: 0 })

const MARGIN = 8

function place() {
  const el = menuRef.value
  const w = el?.offsetWidth || props.width
  const h = el?.offsetHeight || 0
  const maxLeft = window.innerWidth - w - MARGIN
  const maxTop = window.innerHeight - h - MARGIN
  pos.value = {
    left: Math.max(MARGIN, Math.min(props.x, maxLeft)),
    top: Math.max(MARGIN, Math.min(props.y, maxTop)),
  }
}

function onKeydown(event) {
  if (event.key === 'Escape') emit('close')
}

// Pointerdown, not click: a click listener fires after the target's own click
// handler has already run, so a second menu opened from inside this one would
// be torn down by its own opening gesture.
function onPointerDown(event) {
  if (menuRef.value?.contains(event.target)) return
  emit('close')
}

watch(
  () => props.open,
  async (open) => {
    if (!open) {
      teardown()
      return
    }
    pos.value = { left: props.x, top: props.y }
    // Two frames: one for the teleported node to mount, one for it to lay out
    // so offsetWidth/offsetHeight are real before we measure the flip.
    requestAnimationFrame(() => requestAnimationFrame(place))
    window.addEventListener('keydown', onKeydown)
    window.addEventListener('pointerdown', onPointerDown, true)
    window.addEventListener('resize', place)
  },
  { immediate: true },
)

function teardown() {
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('pointerdown', onPointerDown, true)
  window.removeEventListener('resize', place)
}

onBeforeUnmount(teardown)
</script>

<template>
  <Teleport to="body">
    <div
      v-if="props.open"
      ref="menuRef"
      class="ui-menu"
      role="menu"
      :style="{ left: `${pos.left}px`, top: `${pos.top}px`, minWidth: `${props.width}px` }"
    >
      <slot />
    </div>
  </Teleport>
</template>

<style scoped>
.ui-menu {
  position: fixed;
  z-index: 1400;
  padding: var(--gap-1);
  border-radius: var(--r-md);
  background: var(--surface-3);
  box-shadow: var(--shadow-overlay);
  max-height: calc(100vh - 16px);
  overflow: auto;
}

.ui-menu :deep(hr) {
  height: 1px;
  margin: var(--gap-1) 0;
  border: none;
  background: var(--line);
}
</style>
