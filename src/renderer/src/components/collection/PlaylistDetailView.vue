<template>
  <div class="collection__body collection__body--playlist">
    <div :ref="setPlaylistListEl" class="collection__list">
      <div v-if="playlistItems.length === 0" class="collection__empty" style="height: 60px">
        {{ $t('browser.emptyPlaylist') }}
      </div>
      <Table
        :class="{ 'collection__table--reordering': playlistDragFromIdx !== null }"
        v-else
        :on-header-contextmenu="onHeaderContextmenu"
      >
        <template #colgroup>
          <col :style="{ width: TABLE_CHROME_WIDTH.playlistGrip + 'px' }" />
          <col :style="{ width: TABLE_CHROME_WIDTH.playlistIdx + 'px' }" />
          <TableColgroup :fields="store.orderedVisibleColumns" :get-width="columnWidth" />
          <col :style="{ width: TABLE_CHROME_WIDTH.status + 'px' }" />
          <col :style="{ width: TABLE_CHROME_WIDTH.actions + 'px' }" />
          <col :style="{ width: TABLE_CHROME_WIDTH.remove + 'px' }" />
        </template>
        <template #header>
          <TableHeaderCell></TableHeaderCell>
          <TableHeaderCell></TableHeaderCell>
          <TableHeaderCells
            :fields="store.orderedVisibleColumns"
            :get-label="getColumnLabel"
            :dragging-column="draggingColumn"
            :drop-target-column="dropTargetColumn"
            :is-resizable="isResizableField"
            :on-column-header-pointer-down="onColumnHeaderPointerDown"
            :on-resizer-pointer-down="onResizerPointerDown"
            :on-auto-fit-column="autoFitColumn"
          />
          <TableHeaderCell class="table__header-cell--status"></TableHeaderCell>
          <TableHeaderCell align="right">{{ $t('browser.colDecks') }}</TableHeaderCell>
          <TableHeaderCell></TableHeaderCell>
        </template>
        <tr
          v-for="(item, index) in playlistItems"
          :key="item.path"
          class="collection__row collection__playlist-track"
          :class="{
            'collection__playlist-track--dragging': playlistDragFromIdx === index,
            'collection__item--played': mixerStore.playedPaths.has(item.path),
            'collection__item--cursor': isCursor(item.path)
          }"
          :data-row-key="item.path"
          :style="rowShiftStyle(index)"
          @pointerdown="onPlaylistTrackPointerDown($event, index)"
          @dblclick="onTrackDblClickByPath(item.path)"
          @contextmenu.prevent="item.entry && contextMenuEl?.open($event, item.entry.id)"
        >
          <td
            class="collection__td collection__playlist-handle"
            @pointerdown.stop="onGripPointerDown($event, index)"
          >
            <span class="collection__playlist-grip">⠿</span>
          </td>
          <td
            class="collection__td collection__playlist-handle"
            @pointerdown.stop="onGripPointerDown($event, index)"
          >
            <span class="collection__playlist-num">{{ index + 1 }}</span>
          </td>
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
                v-tooltip="field === 'title' ? item.label : (item.entry?.[field] ?? '-')"
                >{{ field === 'title' ? item.label : (item.entry?.[field] ?? '-') }}</span
              >
            </template>
            <template v-else-if="field === 'bpm'">
              <TrackBpmCell
                :status="item.entry?.status ?? null"
                :bpm="item.bpm"
                :on-analyze="() => item.entry && store.analyzeTrack(item.entry.id)"
                :on-set-bpm="() => item.entry && openBpmModal(item.entry.id)"
              />
            </template>
            <template v-else>
              {{ formatAddedDate(item.addedAt) }}
            </template>
          </td>
          <td class="collection__td collection__td--status">
            <TrackStatusTag
              :has-error="
                Boolean(
                  item.entry && (item.entry.status === 'error' || item.entry.lastAnalysisFailed)
                )
              "
            />
          </td>
          <td class="collection__td collection__td--actions">
            <div class="collection__item-actions">
              <Buttons
                :path="item.path"
                :disabled="item.entry === null || item.entry.status !== 'ready'"
              />
            </div>
          </td>
          <td class="collection__td collection__td--remove">
            <button
              class="collection__item-remove"
              tabindex="-1"
              @click.stop="removeFromActivePlaylist(item.path)"
            >
              ✕
            </button>
          </td>
          <td class="collection__td"></td>
        </tr>
      </Table>
    </div>

    <div class="collection__add-section">
      <button
        class="collection__add-toggle"
        tabindex="-1"
        @click="showAddSection = !showAddSection"
      >
        {{ showAddSection ? '▾' : '▸' }} {{ $t('browser.addTracks') }}
      </button>
      <div v-if="showAddSection" class="collection__add-body">
        <Search v-model="addSectionSearch" :full-width="true" />
        <div v-if="addableTracks.length === 0" class="collection__empty" style="height: 40px">
          {{
            store.tracks.filter((t) => t.status === 'ready').length === 0
              ? $t('browser.noAnalyzed')
              : $t('browser.allInPlaylist')
          }}
        </div>
        <div
          v-for="track in addableTracks"
          :key="track.id"
          class="collection__item"
          style="cursor: default"
        >
          <span class="collection__item-name" v-tooltip="track.title ?? displayName(track.name)">{{
            track.title ?? displayName(track.name)
          }}</span>
          <span v-if="store.getBpm(track) !== null" class="collection__item-bpm">
            {{ store.getBpm(track)?.toFixed(1) }} BPM
          </span>
          <button tabindex="-1" class="collection__item-btn" @click="onAddToActivePlaylist(track)">
            +
          </button>
        </div>
      </div>
    </div>

    <BpmModal
      :open="bpmModalTrackId !== null"
      :current-bpm="bpmModalCurrentBpm"
      @submit="onBpmSubmit"
      @cancel="bpmModalTrackId = null"
    />
    <TrackContextMenu ref="contextMenuEl" @set-bpm="openBpmModal" />
    <ColumnVisibilityMenu ref="columnMenuEl" />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, nextTick } from 'vue';
import {
  useCollectionStore,
  isMetadataField,
  type CollectionEntry
} from '@renderer/stores/collection';
import { useDecksStore } from '@renderer/stores/decks';
import { useAppModeStore } from '@renderer/stores/appMode';
import { useMixerStore } from '@renderer/stores/mixer';
import { useElementSize } from '@renderer/composables/useElementSize';
import {
  useColumnResize,
  usePinnedColumnsWidth,
  columnCellClass,
  TABLE_CHROME_WIDTH as BASE_TABLE_CHROME_WIDTH
} from '@renderer/composables/useColumnResize';
import { startTrackDrag } from '@renderer/composables/useTrackDrag';
import { reorderShift } from '@renderer/utils/reorderShift';
import { targetIndexAt } from '@renderer/utils/dropIndex';
import { useBpmModal } from '@renderer/composables/useBpmModal';
import { useRowCursor } from '@renderer/composables/useRowCursor';
import { playlistListId } from '@renderer/stores/browse';
import { useColumnVisibilityMenuTrigger } from '@renderer/composables/useColumnVisibilityMenuTrigger';
import { displayName, formatAddedDate } from '@renderer/utils/trackDisplay';
import { loadToDeck } from '@renderer/utils/deckDrop';
import BpmModal from '@renderer/components/modals/BpmModal.vue';
import Search from '@renderer/components/collection/Search.vue';
import Buttons from '@renderer/components/collection/Buttons.vue';
import Table from '@renderer/components/collection/Table.vue';
import TableColgroup from '@renderer/components/collection/TableColgroup.vue';
import TableHeaderCell from '@renderer/components/collection/TableHeaderCell.vue';
import TableHeaderCells from '@renderer/components/collection/TableHeaderCells.vue';
import TrackBpmCell from '@renderer/components/collection/TrackBpmCell.vue';
import TrackStatusTag from '@renderer/components/collection/TrackStatusTag.vue';
import TrackContextMenu from '@renderer/components/collection/TrackContextMenu.vue';
import ColumnVisibilityMenu from '@renderer/components/collection/ColumnVisibilityMenu.vue';

const props = defineProps<{ playlistId: string }>();

const store = useCollectionStore();
const decksStore = useDecksStore();
const appModeStore = useAppModeStore();
const mixerStore = useMixerStore();

const contextMenuEl = ref<InstanceType<typeof TrackContextMenu> | null>(null);
const { columnMenuEl, onHeaderContextmenu } = useColumnVisibilityMenuTrigger();

// Chrome shared with the main table (status/actions/remove) plus the two
// leading columns unique to the playlist-detail table.
const TABLE_CHROME_WIDTH = { ...BASE_TABLE_CHROME_WIDTH, playlistIdx: 28, playlistGrip: 20 };
const PLAYLIST_DETAIL_FIXED_TOTAL =
  TABLE_CHROME_WIDTH.playlistIdx +
  TABLE_CHROME_WIDTH.playlistGrip +
  TABLE_CHROME_WIDTH.status +
  TABLE_CHROME_WIDTH.actions +
  TABLE_CHROME_WIDTH.remove;

const playlistDetail = useElementSize();
const playlistListEl = playlistDetail.el;
const playlistDetailViewportWidth = playlistDetail.width;
const setPlaylistListEl = playlistDetail.setEl;

const pinnedColumnsWidth = usePinnedColumnsWidth();
const playlistDetailAvailableResizableWidth = () =>
  Math.max(
    0,
    playlistDetailViewportWidth.value - PLAYLIST_DETAIL_FIXED_TOTAL - pinnedColumnsWidth.value
  );

const {
  columnWidth,
  getColumnLabel,
  draggingColumn,
  dropTargetColumn,
  isResizableField,
  onColumnHeaderPointerDown,
  onResizerPointerDown,
  autoFitColumn
} = useColumnResize(playlistDetailAvailableResizableWidth);

const activePlaylist = computed(
  () => store.playlists.find((p) => p.id === props.playlistId) ?? null
);

type PlaylistItem = {
  path: string;
  entry: CollectionEntry | null;
  label: string;
  bpm: number | null;
  addedAt: number | null;
};

const playlistItems = computed((): PlaylistItem[] => {
  const playlist = activePlaylist.value;
  if (!playlist) return [];
  return playlist.paths.map((path) => {
    const entry = store.tracks.find((t) => t.path === path) ?? null;
    const label = entry
      ? (entry.title ?? displayName(entry.name))
      : (path.split('/').pop() ?? path);
    const bpm = entry ? store.getBpm(entry) : null;
    return { path, entry, label, bpm, addedAt: playlist.addedAt[path] ?? null };
  });
});

const { cursorKey, isCursor } = useRowCursor(
  () => playlistListId(props.playlistId),
  () => playlistItems.value.map((item) => item.path)
);

watch(cursorKey, async (key) => {
  if (key === null) return;
  await nextTick();
  const row = playlistListEl.value?.querySelector(`[data-row-key="${CSS.escape(key)}"]`);
  row?.scrollIntoView({ block: 'nearest' });
});

const showAddSection = ref(false);
const addSectionSearch = ref('');

const addableTracks = computed(() => {
  const playlist = activePlaylist.value;
  if (!playlist) return [];
  const q = addSectionSearch.value.trim().toLowerCase();
  return store.tracks.filter((t) => {
    if (!t.path || t.status !== 'ready') return false;
    if (playlist.paths.includes(t.path)) return false;
    if (!q) return true;
    const label = t.title ?? displayName(t.name);
    return label.toLowerCase().includes(q);
  });
});

function removeFromActivePlaylist(path: string) {
  store.removeFromPlaylist(props.playlistId, path);
}

function onAddToActivePlaylist(track: CollectionEntry) {
  if (!track.path) return;
  store.addToPlaylist(props.playlistId, track.path);
}

const playlistDragFromIdx = ref<number | null>(null);
const playlistDropIdx = ref<number | null>(null);
const playlistRowHeight = ref(0);

function rowShiftStyle(index: number) {
  const from = playlistDragFromIdx.value;
  const drop = playlistDropIdx.value;
  if (from === null || drop === null) return undefined;
  const shift = reorderShift(index, from, drop);
  return shift === 0
    ? undefined
    : { transform: `translateY(${shift * playlistRowHeight.value}px)` };
}

function onPlaylistTrackPointerDown(event: PointerEvent, fromIdx: number) {
  const item = playlistItems.value[fromIdx];
  if (item) startTrackDrag(store, event, item.path);
}

function onGripPointerDown(event: PointerEvent, fromIdx: number) {
  if (event.button !== 0) return;
  const playlist = activePlaylist.value;
  if (!playlist) return;
  // A reorder needs WebKit's own autoscroll, or a track cannot be dropped above
  // the visible area. The drag-to-deck path suppresses it instead.
  playlistDragFromIdx.value = fromIdx;
  playlistDropIdx.value = fromIdx;

  const listEl = playlistListEl.value;
  const rows = listEl?.querySelectorAll<HTMLElement>('.collection__playlist-track');
  const firstRow = rows?.[0];
  const rowHeight = firstRow ? firstRow.getBoundingClientRect().height : 0;
  const rowCount = rows ? rows.length : 0;
  const contentTop = firstRow ? firstRow.offsetTop : 0;
  playlistRowHeight.value = rowHeight;
  const grabOffsetY = firstRow
    ? event.clientY - firstRow.getBoundingClientRect().top - fromIdx * rowHeight
    : 0;

  function computeTargetIdx(clientY: number): number {
    if (!listEl || !rows) return fromIdx;
    const listRect = listEl.getBoundingClientRect();
    const withinContent = clientY - listRect.top + listEl.scrollTop;
    return targetIndexAt(withinContent, grabOffsetY, contentTop, rowHeight, rowCount);
  }

  function keepSlotVisible(targetIdx: number) {
    if (!listEl) return;
    const slot = contentTop + targetIdx * rowHeight;
    const top = listEl.scrollTop;
    const bottom = top + listEl.clientHeight - rowHeight;
    if (slot < top) listEl.scrollTop = slot;
    else if (slot > bottom) listEl.scrollTop = slot - listEl.clientHeight + rowHeight;
  }

  function onMove(event: PointerEvent) {
    const targetIdx = computeTargetIdx(event.clientY);
    playlistDropIdx.value = targetIdx;
    keepSlotVisible(targetIdx);
  }

  function resetDrag() {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    window.removeEventListener('pointercancel', onCancel);
    playlistDragFromIdx.value = null;
    playlistDropIdx.value = null;
  }

  function onUp(event: PointerEvent) {
    const from = playlistDragFromIdx.value;
    const targetIdx = computeTargetIdx(event.clientY);
    resetDrag();
    if (from === null || targetIdx === from) return;
    store.moveInPlaylist(playlist?.id ?? null, from, targetIdx);
  }

  function onCancel() {
    resetDrag();
  }

  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onUp);
  window.addEventListener('pointercancel', onCancel);
}

const { bpmModalTrackId, bpmModalCurrentBpm, openBpmModal, onBpmSubmit } = useBpmModal();

function onTrackDblClickByPath(path: string) {
  const entry = store.tracks.find((t) => t.path === path);
  if (!entry || entry.status !== 'ready') return;
  const target = decksStore.bestAvailableDeck(appModeStore.mode === 'edit');
  if (!target) return;
  loadToDeck(path, target);
}
</script>
