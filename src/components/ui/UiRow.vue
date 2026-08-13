<script setup>
/**
 * One line in a list — a collection, a document, a session, a mention result,
 * a recommendation. Six components had grown their own version of this with
 * six slightly different hover washes and paddings.
 *
 * Bordered rows, not rounded cards: in a dense tree, giving every row its own
 * card outline is the fastest way to make a sidebar look busy.
 *
 * `active` is "this row is the current thing"; `selected` is "this row is
 * checked/targeted". They can be true at once, so they layer rather than
 * override.
 */
const props = defineProps({
  interactive: { type: Boolean, default: true },
  active: { type: Boolean, default: false },
  selected: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
  /** sm = tree rows, md = list rows with two lines. */
  size: { type: String, default: 'sm' },
  /** Danger rows in a context menu — delete, remove. */
  tone: { type: String, default: 'default' },
  as: { type: String, default: 'div' },
})

const emit = defineEmits(['click'])

function onClick(event) {
  if (props.disabled) return
  emit('click', event)
}
</script>

<template>
  <component
    :is="props.as"
    class="ui-row"
    :class="[
      `sz-${props.size}`,
      `tone-${props.tone}`,
      {
        interactive: props.interactive && !props.disabled,
        active: props.active,
        selected: props.selected,
        disabled: props.disabled,
      },
    ]"
    @click="onClick"
  >
    <span v-if="$slots.leading" class="ui-row-leading"><slot name="leading" /></span>
    <span class="ui-row-body"><slot /></span>
    <span v-if="$slots.trailing" class="ui-row-trailing"><slot name="trailing" /></span>
  </component>
</template>

<style scoped>
.ui-row {
  display: flex;
  align-items: center;
  gap: var(--gap-3);
  width: 100%;
  min-width: 0;
  border-radius: var(--r-sm);
  color: var(--ink-2);
  font-size: var(--fs-body);
  text-align: left;
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease);
}

.sz-sm {
  padding: 5px var(--gap-3);
  min-height: 26px;
}

.sz-md {
  padding: var(--gap-3) var(--gap-4);
  min-height: 38px;
}

.interactive {
  cursor: pointer;
}

.interactive:hover {
  background: var(--surface-hover);
  color: var(--ink);
}

.active {
  background: var(--surface-hover-strong);
  color: var(--ink);
}

.selected {
  background: var(--accent-tint);
  color: var(--ink);
}

.disabled {
  opacity: 0.45;
  cursor: default;
}

/* The tone only lights up on interaction — a menu full of permanently red text
   reads as an error state rather than as one destructive option. */
.tone-danger.interactive:hover {
  background: var(--danger-tint);
  color: var(--danger);
}

.ui-row-leading,
.ui-row-trailing {
  display: inline-flex;
  align-items: center;
  flex: none;
  color: var(--ink-3);
}

.ui-row-body {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: var(--gap-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
