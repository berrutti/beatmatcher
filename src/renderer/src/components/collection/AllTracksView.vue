<template>
  <div
    :ref="setAllTracksScrollEl"
    class="collection__body"
    @wheel="onAllTracksWheel"
    @scroll.passive="onAllTracksScroll"
  >
    <div v-if="store.tracks.length === 0" class="collection__empty">
      {{ $t('browser.dropHint') }}
    </div>
    <div v-else-if="sortedTracks.length === 0" class="collection__empty">
      {{ $t('browser.noResults') }}
    </div>
    <Table v-else :on-header-contextmenu="onHeaderContextmenu" :thead-ref="setSortBarEl">
      <template #colgroup>
        <TableColgroup :fields="store.orderedVisibleColumns" :get-width="columnWidth" />
        <col :style="{ width: actionsColumnWidth + 'px' }" />
        <col :style="{ width: TABLE_CHROME_WIDTH.remove + 'px' }" />
      </template>
      <template #header>
        <TableHeaderCells
          :fields="store.orderedVisibleColumns"
          :get-label="getColumnLabel"
          :dragging-column="draggingColumn"
          :drop-target-column="dropTargetColumn"
          :is-resizable="isResizableField"
          :on-column-header-pointer-down="onColumnHeaderPointerDown"
          :on-resizer-pointer-down="onResizerPointerDown"
          :on-auto-fit-column="autoFitColumn"
        >
          <template #default="{ field, label }">
            <button tabindex="-1" class="collection__sort-btn" @click.stop="toggleSort(field)">
              {{ label }}{{ sortField === field ? (sortDir === 'asc' ? ' ▲' : ' ▼') : '' }}
            </button>
          </template>
        </TableHeaderCells>
        <TableHeaderCell align="right">{{ $t('browser.colDecks') }}</TableHeaderCell>
        <TableHeaderCell></TableHeaderCell>
      </template>
      <tr v-if="trackRowRange.topSpacerHeight > 0">
        <td :colspan="columnCount" :style="{ height: `${trackRowRange.topSpacerHeight}px` }"></td>
      </tr>
      <tr
        v-for="track in visibleTracks"
        :key="track.id"
        class="collection__row"
        :class="[
          `collection__item--${track.status}`,
          {
            'collection__item--played': track.path && mixerStore.playedPaths.has(track.path),
            'collection__item--cursor': track.path !== null && isCursor(track.path)
          }
        ]"
        @pointerdown="onItemPointerDown($event, track)"
        @dblclick="onTrackDblClick(track)"
        @contextmenu.prevent="contextMenuEl?.open($event, track.id)"
      >
        <td
          v-for="field in store.orderedVisibleColumns"
          :key="field"
          class="collection__td"
          :class="columnCellClass(field)"
        >
          <template v-if="isMetadataField(field)">
            <span
              class="collection__meta-value"
              :class="{ 'collection__meta-value--title': field === 'title' }"
              v-tooltip="metaCellValue(track, field)"
              >{{ metaCellValue(track, field) }}</span
            >
          </template>
          <template v-else-if="field === 'bpm'">
            <TrackBpmCell
              :status="track.status"
              :bpm="store.getBpm(track)"
              :on-analyze="() => store.analyzeTrack(track.id)"
              :on-set-bpm="() => openBpmModal(track.id)"
            />
          </template>
          <template v-else>
            {{ formatAddedDate(track.addedAt) }}
          </template>
        </td>
        <td class="collection__td collection__td--actions">
          <div class="collection__item-actions">
            <span
              v-if="track.status === 'missing'"
              class="collection__item-tag collection__item-tag--missing"
              >{{ $t('browser.statusMissing') }}</span
            >
            <Buttons v-if="track.path" :path="track.path" :disabled="track.status !== 'ready'" />
            <button
              v-if="track.status === 'missing'"
              class="collection__item-btn"
              tabindex="-1"
              @click.stop="store.locateMissingTracks()"
            >
              {{ $t('browser.locate') }}
            </button>
          </div>
        </td>
        <td class="collection__td collection__td--remove">
          <button
            class="collection__item-remove"
            tabindex="-1"
            @click.stop="pendingRemoveTrackId = track.id"
          >
            ✕
          </button>
        </td>
        <td class="collection__td"></td>
      </tr>
      <tr v-if="trackRowRange.bottomSpacerHeight > 0">
        <td
          :colspan="columnCount"
          :style="{ height: `${trackRowRange.bottomSpacerHeight}px` }"
        ></td>
      </tr>
    </Table>

    <BpmModal
      :open="bpmModalTrackId !== null"
      :current-bpm="bpmModalCurrentBpm"
      @submit="onBpmSubmit"
      @cancel="bpmModalTrackId = null"
    />
    <ConfirmModal
      :open="pendingRemoveTrackId !== null"
      :title="$t('browser.removeTrackTitle')"
      :body="$t('browser.removeTrackBody')"
      :confirm-label="$t('browser.remove')"
      @confirm="confirmRemoveTrack"
      @cancel="pendingRemoveTrackId = null"
    />
    <TrackContextMenu ref="contextMenuEl" @set-bpm="openBpmModal" />
    <ColumnVisibilityMenu ref="columnMenuEl" />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, type ComponentPublicInstance } from 'vue';
import {
  useCollectionStore,
  isMetadataField,
  type CollectionEntry,
  type ColumnField,
  type MetadataField
} from '@renderer/stores/collection';
import { useDecksStore } from '@renderer/stores/decks';
import { useAppModeStore } from '@renderer/stores/appMode';
import { useMixerStore } from '@renderer/stores/mixer';
import { useElementSize } from '@renderer/composables/useElementSize';
import {
  useColumnResize,
  usePinnedColumnsWidth,
  metaCellValue,
  columnCellClass,
  TABLE_CHROME_WIDTH
} from '@renderer/composables/useColumnResize';
import { startTrackDrag } from '@renderer/composables/useTrackDrag';
import { useBpmModal } from '@renderer/composables/useBpmModal';
import { useRowCursor } from '@renderer/composables/useRowCursor';
import { useColumnVisibilityMenuTrigger } from '@renderer/composables/useColumnVisibilityMenuTrigger';
import { displayName, formatAddedDate } from '@renderer/utils/trackDisplay';
import { loadToDeck } from '@renderer/utils/deckDrop';
import { useDeckButtons } from '@renderer/composables/useDeckButtons';
import BpmModal from '@renderer/components/modals/BpmModal.vue';
import ConfirmModal from '@renderer/components/modals/ConfirmModal.vue';
import Buttons from '@renderer/components/collection/Buttons.vue';
import Table from '@renderer/components/collection/Table.vue';
import TableColgroup from '@renderer/components/collection/TableColgroup.vue';
import TableHeaderCell from '@renderer/components/collection/TableHeaderCell.vue';
import TableHeaderCells from '@renderer/components/collection/TableHeaderCells.vue';
import TrackBpmCell from '@renderer/components/collection/TrackBpmCell.vue';
import TrackContextMenu from '@renderer/components/collection/TrackContextMenu.vue';
import ColumnVisibilityMenu from '@renderer/components/collection/ColumnVisibilityMenu.vue';

const props = defineProps<{ tracks: CollectionEntry[] }>();

const store = useCollectionStore();
const decksStore = useDecksStore();
const appModeStore = useAppModeStore();
const mixerStore = useMixerStore();

const contextMenuEl = ref<InstanceType<typeof TrackContextMenu> | null>(null);
const { columnMenuEl, onHeaderContextmenu } = useColumnVisibilityMenuTrigger();

const { columnWidth: actionsColumnWidth } = useDeckButtons();

const mainTableFixedTotal = computed(() => actionsColumnWidth.value + TABLE_CHROME_WIDTH.remove);

const allTracks = useElementSize();
const allTracksScrollEl = allTracks.el;
const allTracksViewportHeight = allTracks.height;
const allTracksViewportWidth = allTracks.width;
const setAllTracksScrollEl = allTracks.setEl;

const pinnedColumnsWidth = usePinnedColumnsWidth();
const mainAvailableResizableWidth = () =>
  Math.max(0, allTracksViewportWidth.value - mainTableFixedTotal.value - pinnedColumnsWidth.value);

const {
  columnWidth,
  getColumnLabel,
  draggingColumn,
  dropTargetColumn,
  isResizableField,
  onColumnHeaderPointerDown,
  onResizerPointerDown,
  autoFitColumn
} = useColumnResize(mainAvailableResizableWidth);

type SortField = ColumnField;
const sortField = ref<SortField>('added');
const sortDir = ref<'asc' | 'desc'>('asc');

function toggleSort(field: SortField) {
  if (sortField.value === field) {
    sortDir.value = sortDir.value === 'asc' ? 'desc' : 'asc';
  } else {
    sortField.value = field;
    sortDir.value = 'asc';
  }
}

// Stored as free text, so a lexicographic sort would put "10" before "9".
const NUMERIC_METADATA_FIELDS = ['trackNumber', 'year', 'rating'] as const;

function isNumericMetadataField(field: MetadataField): boolean {
  return NUMERIC_METADATA_FIELDS.some((f) => f === field);
}

// Missing values always sort to the "least" end of the column (oldest, no
// bpm, empty text, unparseable number), regardless of the field's own type.
function sortValue(track: CollectionEntry, field: SortField): string | number {
  if (field === 'added') return track.addedAt ?? -Infinity;
  if (field === 'bpm') return store.getBpm(track) ?? -Infinity;
  if (field === 'title') return (track.title ?? displayName(track.name)).toLowerCase();
  const raw = track[field];
  if (isNumericMetadataField(field)) {
    const parsed = raw !== null ? Number(raw) : NaN;
    return Number.isFinite(parsed) ? parsed : -Infinity;
  }
  return (raw ?? '').toLowerCase();
}

const sortedTracks = computed(() => {
  const tracks = props.tracks;
  // 'added' is insertion order already, so no comparator is needed - just
  // copy (or reverse) instead of paying for a full sort on every recompute.
  if (sortField.value === 'added') {
    return sortDir.value === 'asc' ? [...tracks] : [...tracks].reverse();
  }
  const field = sortField.value;
  return [...tracks].sort((a, b) => {
    const aVal = sortValue(a, field);
    const bVal = sortValue(b, field);
    const cmp = aVal < bVal ? -1 : aVal > bVal ? 1 : 0;
    return sortDir.value === 'asc' ? cmp : -cmp;
  });
});

const TRACK_ROW_HEIGHT = 32; // must match .collection__row height in <style>
const TRACK_ROW_BUFFER = 6;

const allTracksScrollTop = ref(0);
const sortBarHeight = ref(0);

type TemplateRefEl = Element | ComponentPublicInstance | null;

function setSortBarEl(el: TemplateRefEl) {
  sortBarHeight.value = el instanceof HTMLElement ? el.offsetHeight : 0;
}

function onAllTracksScroll() {
  if (allTracksScrollEl.value) allTracksScrollTop.value = allTracksScrollEl.value.scrollTop;
}

// The scroll is blocked rather than overflow-y toggled, which would show and
// hide the scrollbar and shift the table width mid-drag.
function onAllTracksWheel(e: WheelEvent) {
  if (store.draggingPath) e.preventDefault();
}

const trackRowRange = computed(() => {
  const total = sortedTracks.value.length;
  const scrollWithinRows = Math.max(0, allTracksScrollTop.value - sortBarHeight.value);
  const firstVisible = Math.floor(scrollWithinRows / TRACK_ROW_HEIGHT);
  const visibleRowCount = Math.ceil(allTracksViewportHeight.value / TRACK_ROW_HEIGHT);
  const start = Math.max(0, firstVisible - TRACK_ROW_BUFFER);
  const end = Math.min(total, firstVisible + visibleRowCount + TRACK_ROW_BUFFER);
  return {
    start,
    end,
    topSpacerHeight: start * TRACK_ROW_HEIGHT,
    bottomSpacerHeight: (total - end) * TRACK_ROW_HEIGHT
  };
});

const visibleTracks = computed(() =>
  sortedTracks.value.slice(trackRowRange.value.start, trackRowRange.value.end)
);

const { cursorKey, isCursor } = useRowCursor(
  () => 'all',
  () =>
    sortedTracks.value.map((track) => track.path).filter((path): path is string => path !== null)
);

// The rows are virtualized, so the cursor cannot be scrolled to by a DOM call:
// the element it would scroll to does not exist until the scroll has happened.
watch(cursorKey, (key) => {
  const el = allTracksScrollEl.value;
  if (el === null || key === null) return;
  const index = sortedTracks.value.findIndex((track) => track.path === key);
  if (index === -1) return;
  const top = sortBarHeight.value + index * TRACK_ROW_HEIGHT;
  if (top < el.scrollTop) el.scrollTop = top;
  else if (top + TRACK_ROW_HEIGHT > el.scrollTop + allTracksViewportHeight.value)
    el.scrollTop = top + TRACK_ROW_HEIGHT - allTracksViewportHeight.value;
});

// status, actions, remove, the trailing filler column, plus one per visible
// column (metadata + bpm + added).
const columnCount = computed(() => 4 + store.orderedVisibleColumns.length);

const { bpmModalTrackId, bpmModalCurrentBpm, openBpmModal, onBpmSubmit } = useBpmModal();

const pendingRemoveTrackId = ref<string | null>(null);

function confirmRemoveTrack() {
  if (pendingRemoveTrackId.value) store.removeTrack(pendingRemoveTrackId.value);
  pendingRemoveTrackId.value = null;
}

function onTrackDblClick(track: CollectionEntry) {
  if (track.status !== 'ready' || !track.path) return;
  const target = decksStore.bestAvailableDeck(appModeStore.mode === 'edit');
  if (!target) return;
  loadToDeck(track.path, target);
}

// Movement below this threshold is treated as a click, not a drag start.
function onItemPointerDown(event: PointerEvent, track: CollectionEntry) {
  if (track.status !== 'ready' || !track.path) return;
  startTrackDrag(store, event, track.path);
}
</script>
