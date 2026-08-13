<script setup>
/**
 * The bed a form control sits in — search boxes, settings inputs, the rename
 * field, the mention filter. Wrap a bare `<input>`, `<textarea>` or `<select>`
 * and it inherits the field surface, the hairline, and the focus ring.
 *
 * The control keeps its own element (and therefore its own v-model, refs and
 * e2e attributes); this only owns the chrome. That is deliberate — the app has
 * enough focus-management and IME behavior riding on those elements that
 * swallowing them into a wrapper component would break more than it tidies.
 */
const props = defineProps({
  /** sm = inline filters, md = settings forms. */
  size: { type: String, default: 'md' },
  invalid: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
})
</script>

<template>
  <div
    class="ui-field"
    :class="[`sz-${props.size}`, { invalid: props.invalid, disabled: props.disabled }]"
  >
    <span v-if="$slots.leading" class="ui-field-affix"><slot name="leading" /></span>
    <slot />
    <span v-if="$slots.trailing" class="ui-field-affix"><slot name="trailing" /></span>
  </div>
</template>

<style scoped>
.ui-field {
  display: flex;
  align-items: center;
  gap: var(--gap-2);
  width: 100%;
  border-radius: var(--r-sm);
  background: var(--surface-field);
  box-shadow: var(--shadow-hairline);
  color: var(--ink);
  transition: box-shadow var(--dur-fast) var(--ease);
}

.sz-sm {
  min-height: 26px;
  padding: 0 var(--gap-3);
}

.sz-md {
  min-height: 32px;
  padding: 0 var(--gap-4);
}

/* :focus-within rather than :focus — the ring belongs to the field, but the
   focus lands on the control the caller passed in. */
.ui-field:focus-within {
  box-shadow: var(--ring-focus);
}

.invalid {
  box-shadow: 0 0 0 1px var(--danger);
}

.invalid:focus-within {
  box-shadow: 0 0 0 1px var(--danger), 0 0 0 4px var(--danger-tint);
}

.disabled {
  opacity: 0.5;
}

.ui-field-affix {
  display: inline-flex;
  align-items: center;
  flex: none;
  color: var(--ink-3);
}

.ui-field :deep(input),
.ui-field :deep(textarea),
.ui-field :deep(select) {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: transparent;
  color: inherit;
  font: inherit;
  font-size: var(--fs-body-lg);
  padding: 0;
}

.sz-sm :deep(input),
.sz-sm :deep(textarea),
.sz-sm :deep(select) {
  font-size: var(--fs-body);
}

.ui-field :deep(textarea) {
  padding: var(--gap-3) 0;
  resize: vertical;
  line-height: var(--lh-body);
}

.ui-field :deep(::placeholder) {
  color: var(--ink-3);
}
</style>
