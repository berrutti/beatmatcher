<template>
  <th class="table__header-cell" :class="{ 'table__header-cell--right': align === 'right' }">
    <slot />
  </th>
</template>

<script setup lang="ts">
defineProps<{
  align?: 'left' | 'right';
}>();
</script>

<style scoped>
.table__header-cell {
  position: sticky;
  top: 0;
  z-index: 1;
  background: var(--color-surface);
  box-shadow: inset 0 -1px 0 var(--color-border);
  border-right: 1px solid var(--color-border);
  padding: 0.35em 4px;
  font-size: 0.9em;
  letter-spacing: var(--label-letter-spacing);
  color: var(--color-muted);
  font-weight: normal;
  text-align: left;
  text-transform: uppercase;
}

.table__header-cell--right {
  text-align: right;
}

/* Specificity, not just source order, has to beat the base rule above: this
   rule's selector is scoped to the same [data-v-xxx] attribute Vue adds to
   it, so an equivalent override living in another component's global
   stylesheet (lower specificity - no scope attribute) loses to this one
   regardless of which stylesheet loads later. */
.table__header-cell--status {
  border-right: none;
}
</style>
