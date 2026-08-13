<script setup>
/**
 * An icon-only control: the row actions, the composer buttons, the rail.
 *
 * The hit target and the drawing are decoupled on purpose. A 14px stroke icon
 * centered in a 20px (or larger) box keeps the mark visually light while still
 * being reliably clickable — the alternative, scaling the glyph up to fill the
 * target, is what made the old delete button read as enormous next to a 12px
 * row label.
 *
 * `label` is required and becomes the accessible name plus the native tooltip.
 * An icon button without one is unusable to a screen reader and unguessable to
 * everyone else.
 */
const props = defineProps({
  label: { type: String, required: true },
  /** sm = 20px target (dense rows), md = 24px, lg = 28px (toolbars). */
  size: { type: String, default: 'sm' },
  /** default | accent | danger */
  tone: { type: String, default: 'default' },
  active: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
})

const emit = defineEmits(['click'])
</script>

<template>
  <button
    type="button"
    class="ui-icon-btn"
    :class="[`sz-${props.size}`, `tone-${props.tone}`, { active: props.active }]"
    :aria-label="props.label"
    :title="props.label"
    :disabled="props.disabled"
    @click="emit('click', $event)"
  >
    <slot />
  </button>
</template>

<style scoped>
.ui-icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: none;
  padding: 0;
  border: none;
  border-radius: var(--r-sm);
  background: transparent;
  color: var(--ink-3);
  cursor: pointer;
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease);
}

.sz-sm {
  width: 20px;
  height: 20px;
}
.sz-md {
  width: 24px;
  height: 24px;
}
.sz-lg {
  width: 28px;
  height: 28px;
}

/* The drawing stays 14px whatever the target is. */
.ui-icon-btn :deep(svg) {
  width: 14px;
  height: 14px;
  display: block;
}

.sz-lg :deep(svg) {
  width: 16px;
  height: 16px;
}

.ui-icon-btn:hover:not(:disabled) {
  background: var(--surface-hover);
  color: var(--ink);
}

.ui-icon-btn:focus-visible {
  outline: none;
  box-shadow: var(--ring-focus);
}

.active {
  background: var(--surface-hover-strong);
  color: var(--ink);
}

.tone-accent.active,
.tone-accent:hover:not(:disabled) {
  background: var(--accent-tint);
  color: var(--accent-ink);
}

.tone-danger:hover:not(:disabled) {
  background: var(--danger-tint);
  color: var(--danger);
}

.ui-icon-btn:disabled {
  opacity: 0.4;
  cursor: default;
}
</style>
