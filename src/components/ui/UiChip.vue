<script setup>
/**
 * The small pill that carries a piece of status or a reference: an evidence
 * citation, a tool call, a model capability, a mention, a recent source.
 *
 * Six components had their own chip, which is why the app shipped four
 * different reds — each one mixed its own background for "this went wrong".
 * Tones come from the semantic roles so there is exactly one of each.
 */
const props = defineProps({
  /** neutral | accent | success | warning | danger */
  tone: { type: String, default: 'neutral' },
  /** sm = inline with body text, md = standalone. */
  size: { type: String, default: 'sm' },
  /** Filled reads louder; keep it for the one chip that matters in a row. */
  filled: { type: Boolean, default: false },
  interactive: { type: Boolean, default: false },
  as: { type: String, default: 'span' },
})
</script>

<template>
  <component
    :is="props.as"
    class="ui-chip"
    :class="[
      `tone-${props.tone}`,
      `sz-${props.size}`,
      { filled: props.filled, interactive: props.interactive },
    ]"
  >
    <span v-if="$slots.leading" class="ui-chip-leading"><slot name="leading" /></span>
    <slot />
  </component>
</template>

<style scoped>
.ui-chip {
  display: inline-flex;
  align-items: center;
  gap: var(--gap-1);
  max-width: 100%;
  border-radius: var(--r-pill);
  font-weight: var(--w-medium);
  line-height: 1;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease);
}

.sz-sm {
  padding: 3px var(--gap-3);
  font-size: var(--fs-caption);
}

.sz-md {
  padding: 5px var(--gap-4);
  font-size: var(--fs-body);
}

.tone-neutral {
  background: var(--surface-hover);
  color: var(--ink-2);
  box-shadow: var(--shadow-hairline);
}
.tone-accent {
  background: var(--accent-tint);
  color: var(--accent-ink);
}
.tone-success {
  background: var(--success-tint);
  color: var(--success);
}
.tone-warning {
  background: var(--warning-tint);
  color: var(--warning);
}
.tone-danger {
  background: var(--danger-tint);
  color: var(--danger);
}

/* Filled inverts the pairing: a saturated bed needs the far end of the scale,
   not the role color, or it fails contrast. --on-fill rather than --surface-0,
   because the light theme's page floor is a grey and loses half a point. */
.filled.tone-accent {
  background: var(--accent);
  color: var(--on-fill);
}
.filled.tone-success {
  background: var(--success);
  color: var(--on-fill);
}
.filled.tone-warning {
  background: var(--warning);
  color: var(--on-fill);
}
.filled.tone-danger {
  background: var(--danger);
  color: var(--on-fill);
}
.filled.tone-neutral {
  background: var(--surface-field);
  color: var(--ink);
  box-shadow: none;
}

.interactive {
  cursor: pointer;
}

.interactive:hover {
  background: var(--surface-hover-strong);
  color: var(--ink);
}

.ui-chip-leading {
  display: inline-flex;
  align-items: center;
  flex: none;
}
</style>
