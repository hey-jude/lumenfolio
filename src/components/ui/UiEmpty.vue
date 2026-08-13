<script setup>
/**
 * The "nothing here yet" state — an empty library, an empty chat, no saved
 * sessions.
 *
 * Written as an invitation rather than an apology: the title names what this
 * space is for, the copy explains it in one line, and the actions slot is the
 * way out. No "Nothing here yet." — that tells the reader what they can
 * already see.
 */
const props = defineProps({
  title: { type: String, default: '' },
  copy: { type: String, default: '' },
  /** sm = inside a pane, md = a full stage. */
  size: { type: String, default: 'sm' },
})
</script>

<template>
  <div class="ui-empty" :class="`sz-${props.size}`">
    <div v-if="$slots.icon" class="ui-empty-icon" aria-hidden="true"><slot name="icon" /></div>
    <p v-if="props.title" class="ui-empty-title">{{ props.title }}</p>
    <p v-if="props.copy" class="ui-empty-copy">{{ props.copy }}</p>
    <div v-if="$slots.actions" class="ui-empty-actions"><slot name="actions" /></div>
  </div>
</template>

<style scoped>
.ui-empty {
  display: flex;
  flex-direction: column;
  gap: var(--gap-2);
  max-width: 420px;
}

.sz-md {
  gap: var(--gap-3);
  max-width: 520px;
}

.ui-empty-icon {
  display: inline-flex;
  color: var(--ink-3);
  margin-bottom: var(--gap-1);
}

.ui-empty-title {
  margin: 0;
  color: var(--ink);
  font-size: var(--fs-body-lg);
  font-weight: var(--w-strong);
}

.sz-md .ui-empty-title {
  font-size: var(--fs-title);
}

.ui-empty-copy {
  margin: 0;
  color: var(--ink-2);
  font-size: var(--fs-body);
  line-height: var(--lh-body);
}

.ui-empty-actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--gap-3);
  margin-top: var(--gap-2);
}
</style>
