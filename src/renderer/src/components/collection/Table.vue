<template>
  <table class="table" style="width: 100%">
    <colgroup>
      <slot name="colgroup" />
      <!-- Absorbs whatever's left between the real columns and the
           container's width, so the header background and row dividers
           reach the right edge instead of stopping wherever the last real
           column ends. Never wider than 0 once the real columns already
           fill (or exceed) the container - see `width`. -->
      <col />
    </colgroup>
    <thead :ref="onTheadRef">
      <tr class="table__head-row" @contextmenu="onContextmenu">
        <slot name="header" />
        <th class="table__filler"></th>
      </tr>
    </thead>
    <tbody>
      <slot />
    </tbody>
  </table>
</template>

<script setup lang="ts">
import type { ComponentPublicInstance } from 'vue';

type TemplateRefEl = Element | ComponentPublicInstance | null;

const props = defineProps<{
  onHeaderContextmenu?: (e: MouseEvent) => void;
  theadRef?: (el: TemplateRefEl) => void;
}>();

function onContextmenu(e: MouseEvent) {
  if (!props.onHeaderContextmenu) return;
  e.preventDefault();
  props.onHeaderContextmenu(e);
}

function onTheadRef(el: TemplateRefEl) {
  props.theadRef?.(el);
}
</script>

<style scoped>
.table {
  table-layout: fixed;
  border-collapse: collapse;
  font-size: 0.8em;
}

.table__head-row {
  background: var(--color-surface);
}

.table__filler {
  position: sticky;
  top: 0;
  z-index: 1;
  background: var(--color-surface);
  box-shadow: inset 0 -1px 0 var(--color-border);
}
</style>
