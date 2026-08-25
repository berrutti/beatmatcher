<template>
  <TableHeaderCell
    v-for="field in fields"
    :key="field"
    class="table__header-cell--meta"
    :class="{
      'table__header-cell--dragging': draggingColumn === field,
      'table__header-cell--drop-target': dropTargetColumn === field
    }"
    :data-column-field="field"
    @pointerdown="onColumnHeaderPointerDown($event, field)"
  >
    <span class="table__header-cell-content">
      <slot :field="field" :label="getLabel(field)">
        <span class="table__header-cell-label">{{ getLabel(field) }}</span>
      </slot>
    </span>
    <div
      v-if="isResizable(field)"
      class="table__col-resizer"
      @pointerdown.stop="onResizerPointerDown($event, field)"
      @dblclick.stop="onAutoFitColumn(field, $event)"
    ></div>
  </TableHeaderCell>
</template>

<script setup lang="ts" generic="F extends string">
import TableHeaderCell from '@renderer/components/collection/TableHeaderCell.vue';

defineProps<{
  fields: F[];
  getLabel: (field: F) => string;
  draggingColumn: F | null;
  dropTargetColumn: F | null;
  isResizable: (field: F) => boolean;
  onColumnHeaderPointerDown: (e: PointerEvent, field: F) => void;
  onResizerPointerDown: (e: PointerEvent, field: F) => void;
  onAutoFitColumn: (field: F, e: MouseEvent) => void;
}>();
</script>

<style scoped>
.table__header-cell--meta {
  cursor: grab;
  user-select: none;
}

.table__header-cell--meta:active {
  cursor: grabbing;
}

.table__header-cell--dragging {
  opacity: 0.4;
}

.table__header-cell--drop-target {
  background: var(--color-surface);
  box-shadow: inset 0 0 0 1px var(--color-text);
}

/* Truncation lives here rather than on the <th> itself: the resizer handle
   below is a sibling that intentionally sits half outside the <th>'s box
   (see its transform) to straddle the column border, and an overflow:
   hidden on the <th> would clip that half away - taking most of its
   clickable area with it. */
.table__header-cell-content {
  display: block;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  padding-right: 6px;
}

.table__header-cell-label {
  pointer-events: none;
}

.table__col-resizer {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  width: 6px;
  cursor: col-resize;
  transform: translateX(50%);
}
</style>
