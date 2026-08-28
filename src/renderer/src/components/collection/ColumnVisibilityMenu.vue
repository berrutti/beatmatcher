<template>
  <Teleport to="body">
    <div
      v-if="columnMenu"
      ref="columnMenuEl"
      class="context-menu"
      :style="{ left: columnMenu.x + 'px', top: columnMenu.y + 'px' }"
      @click.stop
    >
      <div class="context-menu__title">{{ $t('browser.columnsMenuTitle') }}</div>
      <button
        v-for="field in columnMenuFields"
        :key="field"
        tabindex="-1"
        class="context-menu__item"
        :class="{ 'context-menu__item--disabled': isLastVisibleColumn(field) }"
        v-tooltip="isLastVisibleColumn(field) ? $t('browser.columnRequired') : undefined"
        @click="isLastVisibleColumn(field) || store.toggleColumn(field)"
      >
        <span>{{ COLUMN_LABELS[field] }}</span>
        <span class="context-menu__checkbox">{{ store.isColumnVisible(field) ? '✓' : '' }}</span>
      </button>
    </div>
    <div
      v-if="columnMenu"
      class="context-menu__backdrop"
      @click="close"
      @contextmenu.prevent="close"
    />
  </Teleport>
</template>

<script setup lang="ts">
import { ref, computed, nextTick } from 'vue';
import { useCollectionStore, type ColumnField } from '@renderer/stores/collection';
import { useColumnLabels } from '@renderer/composables/useColumnLabels';
import { clampToViewport } from '@renderer/utils/menuPosition';

const store = useCollectionStore();
const { COLUMN_LABELS } = useColumnLabels();

type ColumnMenu = { x: number; y: number };
const columnMenu = ref<ColumnMenu | null>(null);
const columnMenuEl = ref<HTMLElement | null>(null);

// Alphabetical rather than in table order, so ticking one never shuffles the
// menu under the cursor.
const columnMenuFields = computed<ColumnField[]>(() =>
  [...store.columnOrder].sort((a, b) =>
    COLUMN_LABELS.value[a].localeCompare(COLUMN_LABELS.value[b])
  )
);

function isLastVisibleColumn(field: ColumnField): boolean {
  return store.isColumnVisible(field) && store.orderedVisibleColumns.length === 1;
}

async function open(e: MouseEvent) {
  columnMenu.value = { x: e.clientX, y: e.clientY };
  await nextTick();
  if (!columnMenuEl.value || !columnMenu.value) return;
  const rect = columnMenuEl.value.getBoundingClientRect();
  columnMenu.value = clampToViewport(e, rect);
}

function close() {
  columnMenu.value = null;
}

defineExpose({ open });
</script>
