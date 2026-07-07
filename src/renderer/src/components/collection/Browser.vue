<template>
  <div class="collection" :class="{ 'collection--drag-over': isDragOver }">
    <div class="collection__header">
      <div class="collection__tabs">
        <button
          tabindex="-1"
          class="collection__tab"
          :class="{ 'collection__tab--active': tab === 'all' }"
          @click="tab = 'all'"
        >
          {{ $t('browser.all') }}
        </button>
        <button
          tabindex="-1"
          class="collection__tab"
          :class="{ 'collection__tab--active': tab === 'playlists' }"
          @click="tab = 'playlists'"
        >
          {{ $t('browser.playlists') }}
        </button>
      </div>

      <template v-if="tab === 'all'">
        <span v-if="store.tracks.length > 0" class="collection__count"
          >{{ filteredTracks.length }}/{{ store.tracks.length }}</span
        >
        <Search v-model="searchQuery" />
        <div class="collection__header-actions">
          <button
            tabindex="-1"
            v-if="store.hasPending"
            class="collection__header-btn"
            @click="store.analyzeAll()"
          >
            {{ $t('browser.analyzeAll') }}
          </button>
          <button tabindex="-1" class="collection__header-btn" @click="openFileDialog">
            {{ $t('browser.addFiles') }}
          </button>
          <button tabindex="-1" class="collection__header-btn" @click="openFolderDialog">
            {{ $t('browser.addFolder') }}
          </button>
          <button
            tabindex="-1"
            v-if="store.tracks.length > 0"
            class="collection__header-btn collection__header-btn--muted"
            @click="pendingClear = true"
          >
            {{ $t('browser.clear') }}
          </button>
        </div>
      </template>

      <template v-else-if="activePlaylistId === null">
        <button tabindex="-1" class="collection__header-btn" @click="onCreatePlaylist">
          {{ $t('browser.newPlaylist') }}
        </button>
      </template>

      <template v-else>
        <input
          v-if="renamingPlaylist"
          ref="renameInputEl"
          v-model="renameValue"
          class="collection__playlist-rename"
          spellcheck="false"
          @keydown.enter="confirmRename"
          @keydown.esc="cancelRename"
          @blur="confirmRename"
        />
        <span v-else class="collection__playlist-title" @click="startRename">{{
          activePlaylist?.name
        }}</span>
        <button
          tabindex="-1"
          class="collection__header-btn"
          style="margin-left: 0"
          @click="activePlaylistId = null"
        >
          {{ $t('browser.back') }}
        </button>
      </template>
    </div>

    <div
      v-if="tab === 'all'"
      :ref="setAllTracksScrollEl"
      class="collection__body"
      :style="store.draggingPath ? { overflowY: 'hidden' } : {}"
      @scroll.passive="onAllTracksScroll"
    >
      <div v-if="store.tracks.length === 0" class="collection__empty">
        {{ $t('browser.dropHint') }}
      </div>
      <div v-else-if="sortedFilteredTracks.length === 0" class="collection__empty">
        {{ $t('browser.noResults') }}
      </div>
      <table v-else class="collection__table" :style="{ width: mainTableWidth + 'px' }">
        <colgroup>
          <col
            v-for="field in store.orderedVisibleColumns"
            :key="field"
            :style="{ width: mainMetadataWidths[field] + 'px' }"
          />
          <col :style="{ width: FIXED_COLUMN_WIDTH.status + 'px' }" />
          <col :style="{ width: FIXED_COLUMN_WIDTH.bpm + 'px' }" />
          <col :style="{ width: FIXED_COLUMN_WIDTH.added + 'px' }" />
          <col :style="{ width: FIXED_COLUMN_WIDTH.actions + 'px' }" />
          <col :style="{ width: FIXED_COLUMN_WIDTH.remove + 'px' }" />
        </colgroup>
        <thead :ref="setSortBarEl">
          <tr class="collection__head-row" @contextmenu.prevent="openColumnMenu($event)">
            <th
              v-for="field in store.orderedVisibleColumns"
              :key="field"
              class="collection__th collection__th--meta"
              :class="{
                'collection__th--dragging': draggingColumn === field,
                'collection__th--drop-target': dropTargetColumn === field
              }"
              :data-column-field="field"
              @pointerdown="onColumnHeaderPointerDown($event, field)"
            >
              <button
                v-if="field === 'title'"
                tabindex="-1"
                class="collection__sort-btn"
                @click.stop="toggleSort('title')"
              >
                {{ COLUMN_LABELS[field]
                }}{{ sortField === 'title' ? (sortDir === 'asc' ? ' ▲' : ' ▼') : '' }}
              </button>
              <span v-else class="collection__th-label">{{ COLUMN_LABELS[field] }}</span>
              <div
                class="collection__col-resizer"
                @pointerdown.stop="onResizerPointerDown($event, field)"
                @dblclick.stop="autoFitColumn(field, $event)"
              ></div>
            </th>
            <th class="collection__th"></th>
            <th class="collection__th collection__th--bpm">
              <button tabindex="-1" class="collection__sort-btn" @click="toggleSort('bpm')">
                {{ $t('browser.colBpm')
                }}{{ sortField === 'bpm' ? (sortDir === 'asc' ? ' ▲' : ' ▼') : '' }}
              </button>
            </th>
            <th class="collection__th collection__th--added">
              <button tabindex="-1" class="collection__sort-btn" @click="toggleSort('added')">
                {{ $t('browser.colAdded')
                }}{{ sortField === 'added' ? (sortDir === 'asc' ? ' ▲' : ' ▼') : '' }}
              </button>
            </th>
            <th class="collection__th collection__th--actions">{{ $t('browser.colDecks') }}</th>
            <th class="collection__th collection__th--remove"></th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="trackRowRange.topSpacerHeight > 0">
            <td
              :colspan="columnCount"
              :style="{ height: `${trackRowRange.topSpacerHeight}px` }"
            ></td>
          </tr>
          <tr
            v-for="track in visibleTracks"
            :key="track.id"
            class="collection__row"
            :class="[
              `collection__item--${track.status}`,
              { 'collection__item--played': track.path && mixerStore.playedPaths.has(track.path) }
            ]"
            @pointerdown="onItemPointerDown($event, track)"
            @dblclick="onTrackDblClick(track)"
            @contextmenu.prevent="openContextMenu($event, track.id)"
          >
            <td
              v-for="field in store.orderedVisibleColumns"
              :key="field"
              class="collection__td collection__td--meta"
              @click.stop="startEditCell(track, field)"
            >
              <input
                v-if="
                  editingCell && editingCell.trackId === track.id && editingCell.field === field
                "
                ref="editingCellInputEl"
                v-model="editingCellValue"
                class="collection__meta-input"
                @click.stop
                @keydown.enter="commitEditCell"
                @keydown.esc="cancelEditCell"
                @blur="commitEditCell"
              />
              <span v-else class="collection__meta-value">{{ metaCellValue(track, field) }}</span>
            </td>
            <td class="collection__td collection__td--status">
              <span
                v-if="track.status === 'error' || track.lastAnalysisFailed"
                class="collection__item-tag collection__item-tag--error"
                v-tooltip="$t('browser.analyzeFailedTooltip')"
                >{{ $t('browser.statusError') }}</span
              >
            </td>
            <td class="collection__td collection__td--bpm">
              <span v-if="track.status === 'analyzing'" class="collection__item-tag">
                {{ $t('browser.detecting') }}
              </span>
              <span v-else-if="store.getBpm(track) !== null" class="collection__item-bpm">
                {{ store.getBpm(track)?.toFixed(1) }} BPM
              </span>
              <button
                v-else-if="track.status === 'error'"
                class="collection__item-btn"
                tabindex="-1"
                @click.stop="openBpmModal(track.id)"
              >
                {{ $t('browser.setBpm') }}
              </button>
              <button
                v-else-if="track.status === 'idle'"
                class="collection__item-btn"
                tabindex="-1"
                @click.stop="store.analyzeTrack(track.id)"
              >
                {{ $t('browser.analyze') }}
              </button>
            </td>
            <td class="collection__td collection__td--added">
              {{ formatAddedDate(track.addedAt) }}
            </td>
            <td class="collection__td collection__td--actions">
              <div class="collection__item-actions">
                <span
                  v-if="track.status === 'missing'"
                  class="collection__item-tag collection__item-tag--missing"
                  >{{ $t('browser.statusMissing') }}</span
                >
                <Buttons
                  v-if="track.path"
                  :path="track.path"
                  :disabled="track.status !== 'ready'"
                />
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
          </tr>
          <tr v-if="trackRowRange.bottomSpacerHeight > 0">
            <td
              :colspan="columnCount"
              :style="{ height: `${trackRowRange.bottomSpacerHeight}px` }"
            ></td>
          </tr>
        </tbody>
      </table>
    </div>

    <div
      v-else-if="activePlaylistId === null"
      :ref="setPlaylistsOverviewEl"
      class="collection__body"
    >
      <div v-if="store.playlists.length === 0" class="collection__empty">
        {{ $t('browser.noPlaylists') }}
      </div>
      <table v-else class="collection__table" :style="{ width: playlistListTableWidth + 'px' }">
        <colgroup>
          <col
            v-for="field in playlistListColumnsState.order"
            :key="field"
            :style="{ width: playlistListWidths[field] + 'px' }"
          />
          <col :style="{ width: FIXED_COLUMN_WIDTH.remove + 'px' }" />
        </colgroup>
        <thead>
          <tr class="collection__head-row">
            <th
              v-for="field in playlistListColumnsState.order"
              :key="field"
              class="collection__th collection__th--meta"
              :class="{
                'collection__th--dragging': draggingPlaylistListColumn === field,
                'collection__th--drop-target': dropTargetPlaylistListColumn === field
              }"
              :data-column-field="field"
              @pointerdown="onPlaylistListColumnHeaderPointerDown($event, field)"
            >
              <span class="collection__th-label">{{ PLAYLIST_LIST_COLUMN_LABELS[field] }}</span>
              <div
                class="collection__col-resizer"
                @pointerdown.stop="onPlaylistListResizerPointerDown($event, field)"
                @dblclick.stop="autoFitPlaylistListColumn(field, $event)"
              ></div>
            </th>
            <th class="collection__th collection__th--remove"></th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="playlist in store.playlists"
            :key="playlist.id"
            class="collection__row collection__item--playlist"
            @click="openPlaylist(playlist.id)"
          >
            <td
              v-for="field in playlistListColumnsState.order"
              :key="field"
              class="collection__td"
              :class="{
                'collection__td--title': field === 'title',
                'collection__td--bpm': field === 'tracks'
              }"
            >
              <span v-if="field === 'title'" class="collection__item-name">{{
                playlist.name
              }}</span>
              <span v-else>{{ $t('browser.trackCount', playlist.paths.length) }}</span>
            </td>
            <td class="collection__td collection__td--remove">
              <button
                class="collection__item-remove"
                tabindex="-1"
                @click.stop="pendingDeletePlaylistId = playlist.id"
              >
                ✕
              </button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-else class="collection__body collection__body--playlist">
      <div :ref="setPlaylistListEl" class="collection__list">
        <div v-if="playlistItems.length === 0" class="collection__empty" style="height: 60px">
          {{ $t('browser.emptyPlaylist') }}
        </div>
        <table v-else class="collection__table" :style="{ width: playlistDetailTableWidth + 'px' }">
          <colgroup>
            <col :style="{ width: FIXED_COLUMN_WIDTH.playlistIdx + 'px' }" />
            <col :style="{ width: FIXED_COLUMN_WIDTH.playlistGrip + 'px' }" />
            <col
              v-for="field in store.orderedVisibleColumns"
              :key="field"
              :style="{ width: playlistDetailMetadataWidths[field] + 'px' }"
            />
            <col :style="{ width: FIXED_COLUMN_WIDTH.bpm + 'px' }" />
            <col :style="{ width: FIXED_COLUMN_WIDTH.added + 'px' }" />
            <col :style="{ width: FIXED_COLUMN_WIDTH.actions + 'px' }" />
            <col :style="{ width: FIXED_COLUMN_WIDTH.remove + 'px' }" />
          </colgroup>
          <thead>
            <tr class="collection__head-row" @contextmenu.prevent="openColumnMenu($event)">
              <th class="collection__th"></th>
              <th class="collection__th"></th>
              <th
                v-for="field in store.orderedVisibleColumns"
                :key="field"
                class="collection__th collection__th--meta"
                :class="{
                  'collection__th--dragging': draggingColumn === field,
                  'collection__th--drop-target': dropTargetColumn === field
                }"
                :data-column-field="field"
                @pointerdown="onColumnHeaderPointerDown($event, field)"
              >
                <span class="collection__th-label">{{ COLUMN_LABELS[field] }}</span>
                <div
                  class="collection__col-resizer"
                  @pointerdown.stop="onResizerPointerDown($event, field)"
                  @dblclick.stop="autoFitColumn(field, $event)"
                ></div>
              </th>
              <th class="collection__th collection__th--bpm">{{ $t('browser.colBpm') }}</th>
              <th class="collection__th collection__th--added">{{ $t('browser.colAdded') }}</th>
              <th class="collection__th collection__th--actions">{{ $t('browser.colDecks') }}</th>
              <th class="collection__th collection__th--remove"></th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="(item, idx) in playlistItems"
              :key="item.path"
              class="collection__row collection__playlist-track"
              :class="{
                'collection__playlist-track--dragging': playlistDragFromIdx === idx,
                'collection__item--played': mixerStore.playedPaths.has(item.path)
              }"
              @pointerdown="onPlaylistTrackPointerDown($event, idx)"
              @dblclick="onTrackDblClickByPath(item.path)"
              @contextmenu.prevent="item.entry && openContextMenu($event, item.entry.id)"
            >
              <td class="collection__td">
                <span class="collection__playlist-num">{{ idx + 1 }}</span>
              </td>
              <td class="collection__td">
                <span class="collection__playlist-grip">⠿</span>
              </td>
              <td
                v-for="field in store.orderedVisibleColumns"
                :key="field"
                class="collection__td collection__td--meta"
                @click.stop="item.entry && startEditCell(item.entry, field)"
              >
                <input
                  v-if="
                    item.entry &&
                    editingCell &&
                    editingCell.trackId === item.entry.id &&
                    editingCell.field === field
                  "
                  ref="editingCellInputEl"
                  v-model="editingCellValue"
                  class="collection__meta-input"
                  @click.stop
                  @keydown.enter="commitEditCell"
                  @keydown.esc="cancelEditCell"
                  @blur="commitEditCell"
                />
                <span v-else class="collection__meta-value">{{
                  field === 'title' ? item.label : (item.entry?.[field] ?? '—')
                }}</span>
              </td>
              <td class="collection__td collection__td--bpm">
                <span v-if="item.bpm !== null" class="collection__item-bpm"
                  >{{ item.bpm.toFixed(1) }} BPM</span
                >
              </td>
              <td class="collection__td collection__td--added">
                {{ formatAddedDate(item.addedAt) }}
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
            </tr>
          </tbody>
        </table>
        <div
          v-if="showDropLine"
          class="collection__drop-line"
          :style="{ top: `${playlistDropY - 1}px` }"
        />
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
            <span class="collection__item-name">{{ track.title ?? displayName(track.name) }}</span>
            <span v-if="store.getBpm(track) !== null" class="collection__item-bpm">
              {{ store.getBpm(track)?.toFixed(1) }} BPM
            </span>
            <button
              tabindex="-1"
              class="collection__item-btn"
              @click="onAddToActivePlaylist(track)"
            >
              +
            </button>
          </div>
        </div>
      </div>
    </div>

    <BpmModal
      :open="bpmModalTrackId !== null"
      :current-bpm="bpmModalCurrentBpm"
      @submit="onBpmSubmit"
      @cancel="bpmModalTrackId = null"
    />
    <ConfirmModal
      :open="pendingClear"
      :title="$t('browser.clearTitle')"
      :body="$t('browser.clearBody')"
      :confirm-label="$t('browser.clearConfirm')"
      @confirm="confirmClear"
      @cancel="pendingClear = false"
    />
    <ConfirmModal
      :open="pendingDeletePlaylistId !== null"
      :title="$t('browser.deletePlaylistTitle', { name: pendingDeletePlaylistName })"
      :body="$t('browser.deletePlaylistBody')"
      :confirm-label="$t('browser.delete')"
      @confirm="confirmDeletePlaylist"
      @cancel="pendingDeletePlaylistId = null"
    />
    <ConfirmModal
      :open="pendingRemoveTrackId !== null"
      :title="$t('browser.removeTrackTitle')"
      :body="$t('browser.removeTrackBody')"
      :confirm-label="$t('browser.remove')"
      @confirm="confirmRemoveTrack"
      @cancel="pendingRemoveTrackId = null"
    />

    <Teleport to="body">
      <div
        v-if="contextMenu"
        ref="contextMenuEl"
        class="context-menu"
        :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px' }"
        @click.stop
      >
        <button tabindex="-1" class="context-menu__item" @click="onContextMenuReanalyze">
          {{ $t('browser.recalcBpm') }}
        </button>
        <button tabindex="-1" class="context-menu__item" @click="onContextMenuSetBpm">
          {{ $t('browser.setBpm') }}
        </button>
        <div
          v-if="store.playlists.length > 0"
          class="context-menu__item context-menu__item--sub"
          @mouseenter="onSubEnter"
        >
          <span>{{ $t('browser.addToPlaylist') }}</span>
          <span class="context-menu__arrow">▶</span>
          <div class="context-menu__submenu" :class="{ 'context-menu__submenu--flip': subFlipped }">
            <button
              tabindex="-1"
              v-for="playlist in store.playlists"
              :key="playlist.id"
              class="context-menu__item"
              @click="onContextMenuAddToPlaylist(playlist.id)"
            >
              {{ playlist.name }}
            </button>
          </div>
        </div>
        <div v-else class="context-menu__item context-menu__item--disabled">
          <span>{{ $t('browser.addToPlaylist') }}</span>
          <span class="context-menu__item-hint">{{ $t('browser.noPlaylistsShort') }}</span>
        </div>
      </div>
      <div
        v-if="contextMenu"
        class="context-menu__backdrop"
        @click="closeContextMenu"
        @contextmenu.prevent="closeContextMenu"
      />
    </Teleport>

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
          <span class="context-menu__checkbox">{{ store.isColumnVisible(field) ? '✓' : '' }}</span>
          <span>{{ COLUMN_LABELS[field] }}</span>
        </button>
      </div>
      <div
        v-if="columnMenu"
        class="context-menu__backdrop"
        @click="closeColumnMenu"
        @contextmenu.prevent="closeColumnMenu"
      />
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import {
  ref,
  reactive,
  computed,
  nextTick,
  onMounted,
  onUnmounted,
  type ComponentPublicInstance
} from 'vue';
import { useI18n } from 'vue-i18n';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { useCollectionStore, isMetadataField } from '@renderer/stores/collection';
import { useDecksStore } from '@renderer/stores/decks';
import { useAppModeStore } from '@renderer/stores/appMode';
import { useMixerStore } from '@renderer/stores/mixer';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import { distributeColumnWidths } from '@renderer/utils/columnLayout';
import type { CollectionEntry, MetadataField } from '@renderer/stores/collection';
import BpmModal from '@renderer/components/modals/BpmModal.vue';
import ConfirmModal from '@renderer/components/modals/ConfirmModal.vue';
import Search from '@renderer/components/collection/Search.vue';
import Buttons from '@renderer/components/collection/Buttons.vue';

const { t } = useI18n();
const store = useCollectionStore();
const decksStore = useDecksStore();
const appModeStore = useAppModeStore();
const mixerStore = useMixerStore();

const isDragOver = ref(false);
const pendingClear = ref(false);
const bpmModalTrackId = ref<string | null>(null);
const bpmModalCurrentBpm = computed(() => {
  const track = store.tracks.find((t) => t.id === bpmModalTrackId.value);
  return track ? store.getBpm(track) : null;
});
const searchQuery = ref('');

const tab = ref<'all' | 'playlists'>('all');
const activePlaylistId = ref<string | null>(null);
const renamingPlaylist = ref(false);
const renameValue = ref('');
const renameInputEl = ref<HTMLInputElement | null>(null);
const showAddSection = ref(false);
const addSectionSearch = ref('');

const playlistListEl = ref<HTMLElement | null>(null);
const playlistDetailViewportWidth = ref(0);
let playlistDetailResizeObserver: ResizeObserver | null = null;

function setPlaylistListEl(el: TemplateRefEl) {
  playlistDetailResizeObserver?.disconnect();
  playlistDetailResizeObserver = null;
  const target = el instanceof HTMLElement ? el : null;
  playlistListEl.value = target;
  if (!target) return;
  playlistDetailViewportWidth.value = target.clientWidth;
  playlistDetailResizeObserver = new ResizeObserver(() => {
    playlistDetailViewportWidth.value = target.clientWidth;
  });
  playlistDetailResizeObserver.observe(target);
}
onUnmounted(() => playlistDetailResizeObserver?.disconnect());

const playlistsOverviewViewportWidth = ref(0);
let playlistsOverviewResizeObserver: ResizeObserver | null = null;

function setPlaylistsOverviewEl(el: TemplateRefEl) {
  playlistsOverviewResizeObserver?.disconnect();
  playlistsOverviewResizeObserver = null;
  const target = el instanceof HTMLElement ? el : null;
  if (!target) return;
  playlistsOverviewViewportWidth.value = target.clientWidth;
  playlistsOverviewResizeObserver = new ResizeObserver(() => {
    playlistsOverviewViewportWidth.value = target.clientWidth;
  });
  playlistsOverviewResizeObserver.observe(target);
}
onUnmounted(() => playlistsOverviewResizeObserver?.disconnect());

const playlistDragFromIdx = ref<number | null>(null);
const playlistDropIdx = ref<number | null>(null);
const playlistDropY = ref(0);

type SortField = 'title' | 'bpm' | 'added';
const sortField = ref<SortField>('added');
const sortDir = ref<'asc' | 'desc'>('asc');
const pendingDeletePlaylistId = ref<string | null>(null);
const pendingRemoveTrackId = ref<string | null>(null);

const pendingDeletePlaylistName = computed(
  () => store.playlists.find((p) => p.id === pendingDeletePlaylistId.value)?.name ?? ''
);

type ContextMenu = { trackId: string; x: number; y: number };
const contextMenu = ref<ContextMenu | null>(null);
const contextMenuEl = ref<HTMLElement | null>(null);
const subFlipped = ref(false);

async function openContextMenu(e: MouseEvent, trackId: string) {
  contextMenu.value = { trackId, x: e.clientX, y: e.clientY };
  await nextTick();
  if (!contextMenuEl.value || !contextMenu.value) return;
  const rect = contextMenuEl.value.getBoundingClientRect();
  const x = rect.right > window.innerWidth ? e.clientX - rect.width : e.clientX;
  const y = rect.bottom > window.innerHeight ? e.clientY - rect.height : e.clientY;
  contextMenu.value = { ...contextMenu.value, x, y };
}

function onSubEnter(e: MouseEvent) {
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
  const submenuHeight = store.playlists.length * 32 + 8;
  subFlipped.value = rect.top + submenuHeight > window.innerHeight;
}

function closeContextMenu() {
  contextMenu.value = null;
}

function onContextMenuReanalyze() {
  if (contextMenu.value) store.reanalyzeTrack(contextMenu.value.trackId);
  closeContextMenu();
}

function onContextMenuSetBpm() {
  if (contextMenu.value) openBpmModal(contextMenu.value.trackId);
  closeContextMenu();
}

function onContextMenuAddToPlaylist(playlistId: string) {
  const track = store.tracks.find((t) => t.id === contextMenu.value?.trackId);
  if (track?.path) store.addToPlaylist(playlistId, track.path);
  closeContextMenu();
}

const COLUMN_LABELS = computed<Record<MetadataField, string>>(() => ({
  title: t('browser.colTitle'),
  artist: t('browser.colArtist'),
  album: t('browser.colAlbum'),
  albumArtist: t('browser.colAlbumArtist'),
  genre: t('browser.colGenre'),
  composer: t('browser.colComposer'),
  remixer: t('browser.colRemixer'),
  label: t('browser.colLabel'),
  comment: t('browser.colComment'),
  trackNumber: t('browser.colTrackNumber'),
  year: t('browser.colYear'),
  rating: t('browser.colRating')
}));

// The column picker menu lists columns alphabetically regardless of their
// drag-reordered position in the table, so toggling a column's visibility
// never shuffles the menu itself.
const columnMenuFields = computed<MetadataField[]>(() =>
  [...store.columnOrder].sort((a, b) =>
    COLUMN_LABELS.value[a].localeCompare(COLUMN_LABELS.value[b])
  )
);

function isLastVisibleColumn(field: MetadataField): boolean {
  return store.isColumnVisible(field) && store.orderedVisibleColumns.length === 1;
}

type ColumnMenu = { x: number; y: number };
const columnMenu = ref<ColumnMenu | null>(null);
const columnMenuEl = ref<HTMLElement | null>(null);

async function openColumnMenu(e: MouseEvent) {
  columnMenu.value = { x: e.clientX, y: e.clientY };
  await nextTick();
  if (!columnMenuEl.value || !columnMenu.value) return;
  const rect = columnMenuEl.value.getBoundingClientRect();
  const x = rect.right > window.innerWidth ? e.clientX - rect.width : e.clientX;
  const y = rect.bottom > window.innerHeight ? e.clientY - rect.height : e.clientY;
  columnMenu.value = { x, y };
}

function closeColumnMenu() {
  columnMenu.value = null;
}

function metaCellValue(track: CollectionEntry, field: MetadataField): string {
  if (field === 'title') return track.title ?? displayName(track.name);
  return track[field] ?? '—';
}

const COLUMN_DRAG_THRESHOLD = 5;
let autoFitCanvas: HTMLCanvasElement | null = null;
const AUTO_FIT_PADDING = 24;

// Widths for columns that are never resizable, so a table's leftover space
// (container width minus these) is what the resizable metadata columns grow
// into, instead of these getting stretched along with them.
const FIXED_COLUMN_WIDTH = {
  status: 60,
  bpm: 55,
  added: 55,
  actions: 180,
  remove: 32,
  playlistIdx: 28,
  playlistGrip: 20
};

type ColumnDragOptions<F extends string> = {
  getWidth: (field: F) => number;
  setWidth: (field: F, widthPx: number) => void;
  reorder: (field: F, beforeField: F | null) => void;
  isField: (value: string | undefined) => value is F;
  getLabel: (field: F) => string;
};

// Shared by every resizable/reorderable header row in this component (the
// metadata columns and the playlist-list columns), each with its own backing
// state. The header row is found via `closest` at drag start, so one
// instance works across as many tables as call into it, with no per-table
// element ref needed.
function useColumnDrag<F extends string>(options: ColumnDragOptions<F>) {
  const draggingColumn = ref<F | null>(null);
  const dropTargetColumn = ref<F | null>(null);
  const resizingColumn = ref<F | null>(null);

  function onColumnHeaderPointerDown(e: PointerEvent, field: F) {
    if (e.button !== 0) return;
    const headerRow = (e.currentTarget as HTMLElement).closest('tr');
    if (!headerRow) return;
    const startX = e.clientX;
    let active = false;

    function orderedFields(): F[] {
      if (!headerRow) return [];
      return Array.from(headerRow.querySelectorAll<HTMLElement>('[data-column-field]'))
        .map((el) => el.dataset.columnField)
        .filter(options.isField);
    }

    // Column positions shift the instant a swap is applied (the table
    // re-renders in the new order), so re-measuring live rects on every move
    // would see the just-swapped neighbor sitting back under the still
    // motionless cursor and immediately swap back. Slot boundaries are
    // measured once, before anything moves, and only re-used to ask "which
    // field currently occupies this slot" as the order changes underneath.
    const slotRects = Array.from(
      headerRow.querySelectorAll<HTMLElement>('[data-column-field]')
    ).map((el) => el.getBoundingClientRect());

    function slotIndexAt(clientX: number): number {
      for (let i = 0; i < slotRects.length; i++) {
        if (clientX < slotRects[i].left + slotRects[i].width / 2) return i;
      }
      return slotRects.length - 1;
    }

    // The swap itself only happens on drop; while dragging this just tracks
    // what would happen so the drop target can be highlighted live.
    let pendingBefore: F | null = null;
    let hasPendingSwap = false;

    function onMove(ev: PointerEvent) {
      if (!active) {
        if (Math.abs(ev.clientX - startX) < COLUMN_DRAG_THRESHOLD) return;
        active = true;
        draggingColumn.value = field;
      }
      const fields = orderedFields();
      const draggedIndex = fields.indexOf(field);
      const slot = slotIndexAt(ev.clientX);
      const target = fields[slot];
      if (draggedIndex === -1 || target === undefined || target === field) {
        dropTargetColumn.value = null;
        hasPendingSwap = false;
        return;
      }
      dropTargetColumn.value = target;
      pendingBefore = slot < draggedIndex ? target : (fields[slot + 1] ?? null);
      hasPendingSwap = true;
    }

    function stopListeners() {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      window.removeEventListener('pointercancel', onCancel);
    }

    function onUp() {
      stopListeners();
      if (active && hasPendingSwap) options.reorder(field, pendingBefore);
      draggingColumn.value = null;
      dropTargetColumn.value = null;
    }

    function onCancel() {
      stopListeners();
      draggingColumn.value = null;
      dropTargetColumn.value = null;
    }

    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onCancel);
  }

  function onResizerPointerDown(e: PointerEvent, field: F) {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const startWidth = options.getWidth(field);
    resizingColumn.value = field;

    function onMove(ev: PointerEvent) {
      options.setWidth(field, startWidth + (ev.clientX - startX));
    }

    function stop() {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', stop);
      window.removeEventListener('pointercancel', stop);
      resizingColumn.value = null;
    }

    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', stop);
    window.addEventListener('pointercancel', stop);
  }

  function autoFitColumn(field: F, e: MouseEvent) {
    const th = (e.currentTarget as HTMLElement).closest('th');
    if (!autoFitCanvas) autoFitCanvas = document.createElement('canvas');
    const ctx = autoFitCanvas.getContext('2d');
    if (!ctx) return;
    ctx.font = th ? getComputedStyle(th).font : getComputedStyle(document.body).font;
    const textWidth = ctx.measureText(options.getLabel(field)).width;
    options.setWidth(field, Math.ceil(textWidth) + AUTO_FIT_PADDING);
  }

  return {
    draggingColumn,
    dropTargetColumn,
    resizingColumn,
    onColumnHeaderPointerDown,
    onResizerPointerDown,
    autoFitColumn
  };
}

const {
  draggingColumn,
  dropTargetColumn,
  resizingColumn,
  onColumnHeaderPointerDown,
  onResizerPointerDown,
  autoFitColumn
} = useColumnDrag<MetadataField>({
  getWidth: store.getColumnWidth,
  setWidth: store.setColumnWidth,
  reorder: store.reorderColumn,
  isField: isMetadataField,
  getLabel: (field) => COLUMN_LABELS.value[field]
});

// The playlists overview table (Title/Tracks) isn't sortable by header click
// like the track tables are, but still gets resize + reorder on its two
// columns, via its own small, separately persisted order/width state.
type PlaylistListColumnField = 'title' | 'tracks';
const PLAYLIST_LIST_COLUMN_FIELDS: PlaylistListColumnField[] = ['title', 'tracks'];
const PLAYLIST_LIST_DEFAULT_WIDTH: Record<PlaylistListColumnField, number> = {
  title: 200,
  tracks: 90
};

function isPlaylistListColumnField(value: string | undefined): value is PlaylistListColumnField {
  return value === 'title' || value === 'tracks';
}

type PlaylistListColumnsState = {
  order: PlaylistListColumnField[];
  widths: Partial<Record<PlaylistListColumnField, number>>;
};

function loadPlaylistListColumnsState(): PlaylistListColumnsState {
  const fallback: PlaylistListColumnsState = {
    order: [...PLAYLIST_LIST_COLUMN_FIELDS],
    widths: {}
  };
  const stored = storageGet<Partial<PlaylistListColumnsState> | null>(
    STORAGE_KEYS.playlistListColumns,
    null
  );
  if (!stored || !Array.isArray(stored.order)) return fallback;
  const validOrder = stored.order.filter(isPlaylistListColumnField);
  const order = [
    ...validOrder,
    ...PLAYLIST_LIST_COLUMN_FIELDS.filter((f) => !validOrder.includes(f))
  ];
  return { order, widths: stored.widths ?? {} };
}

const playlistListColumnsState = reactive<PlaylistListColumnsState>(loadPlaylistListColumnsState());

function persistPlaylistListColumnsState() {
  storageSet(STORAGE_KEYS.playlistListColumns, playlistListColumnsState);
}

function getPlaylistListColumnWidth(field: PlaylistListColumnField): number {
  return playlistListColumnsState.widths[field] ?? PLAYLIST_LIST_DEFAULT_WIDTH[field];
}

function setPlaylistListColumnWidth(field: PlaylistListColumnField, widthPx: number) {
  playlistListColumnsState.widths[field] = Math.max(40, Math.round(widthPx));
  persistPlaylistListColumnsState();
}

function reorderPlaylistListColumn(
  field: PlaylistListColumnField,
  beforeField: PlaylistListColumnField | null
) {
  const order = playlistListColumnsState.order;
  const fromIndex = order.indexOf(field);
  if (fromIndex === -1) return;
  order.splice(fromIndex, 1);
  const toIndex = beforeField !== null ? order.indexOf(beforeField) : order.length;
  order.splice(toIndex, 0, field);
  persistPlaylistListColumnsState();
}

const PLAYLIST_LIST_COLUMN_LABELS = computed<Record<PlaylistListColumnField, string>>(() => ({
  title: t('browser.colTitle'),
  tracks: t('browser.colTracks')
}));

const {
  draggingColumn: draggingPlaylistListColumn,
  dropTargetColumn: dropTargetPlaylistListColumn,
  resizingColumn: resizingPlaylistListColumn,
  onColumnHeaderPointerDown: onPlaylistListColumnHeaderPointerDown,
  onResizerPointerDown: onPlaylistListResizerPointerDown,
  autoFitColumn: autoFitPlaylistListColumn
} = useColumnDrag<PlaylistListColumnField>({
  getWidth: getPlaylistListColumnWidth,
  setWidth: setPlaylistListColumnWidth,
  reorder: reorderPlaylistListColumn,
  isField: isPlaylistListColumnField,
  getLabel: (field) => PLAYLIST_LIST_COLUMN_LABELS.value[field]
});

type EditingCell = { trackId: string; field: MetadataField };
const editingCell = ref<EditingCell | null>(null);
const editingCellValue = ref('');
// Bound with ref="editingCellInputEl" inside a v-for, so Vue collects it as
// an array even though at most one input is ever rendered at a time.
const editingCellInputEl = ref<HTMLInputElement[]>([]);

async function startEditCell(track: CollectionEntry, field: MetadataField) {
  if (!track.path) return;
  editingCell.value = { trackId: track.id, field };
  editingCellValue.value = track[field] ?? '';
  await nextTick();
  const inputEl = editingCellInputEl.value[0];
  inputEl?.focus();
  inputEl?.select();
}

function commitEditCell() {
  if (!editingCell.value) return;
  const { trackId, field } = editingCell.value;
  const value = editingCellValue.value.trim();
  store.setMetadataField(trackId, field, value.length > 0 ? value : null);
  editingCell.value = null;
}

function cancelEditCell() {
  editingCell.value = null;
}

function toggleSort(field: SortField) {
  if (sortField.value === field) {
    sortDir.value = sortDir.value === 'asc' ? 'desc' : 'asc';
  } else {
    sortField.value = field;
    sortDir.value = 'asc';
  }
}

const filteredTracks = computed(() => {
  const q = searchQuery.value.trim().toLowerCase();
  if (!q) return store.tracks;
  return store.tracks.filter((t) => {
    const label = t.title ?? displayName(t.name);
    return label.toLowerCase().includes(q);
  });
});

const sortedFilteredTracks = computed(() => {
  const tracks = filteredTracks.value;
  return [...tracks].sort((a, b) => {
    let aVal: string | number;
    let bVal: string | number;
    if (sortField.value === 'title') {
      aVal = (a.title ?? displayName(a.name)).toLowerCase();
      bVal = (b.title ?? displayName(b.name)).toLowerCase();
    } else if (sortField.value === 'bpm') {
      // Tracks with no BPM yet sort as if they were the slowest, so they
      // always land at the "no bpm < slow < fast" end of the column.
      aVal = store.getBpm(a) ?? -Infinity;
      bVal = store.getBpm(b) ?? -Infinity;
    } else {
      // Tracks with no known addedAt sort as the oldest.
      aVal = a.addedAt ?? -Infinity;
      bVal = b.addedAt ?? -Infinity;
    }
    const cmp = aVal < bVal ? -1 : aVal > bVal ? 1 : 0;
    return sortDir.value === 'asc' ? cmp : -cmp;
  });
});

// Playlist items have a null addedAt when the playlist was saved before
// per-playlist "date added" existed, so there's genuinely nothing recorded.
function formatAddedDate(addedAt: number | null): string {
  if (addedAt === null) return '—';
  return new Date(addedAt).toLocaleDateString(undefined, {
    year: '2-digit',
    month: '2-digit',
    day: '2-digit'
  });
}

// Large collections (hundreds of tracks) made every row a permanent DOM node,
// so resizing the window forced a full flex/text-ellipsis layout pass over
// all of them every frame, even the ones scrolled out of view. Only rows
// within (or near) the visible scroll area are mounted; the rest are
// represented by two spacer divs sized to the height they'd otherwise take up.
const TRACK_ROW_HEIGHT = 32; // must match .collection__row height in <style>
const TRACK_ROW_BUFFER = 6;

type TemplateRefEl = Element | ComponentPublicInstance | null;

const allTracksScrollEl = ref<HTMLElement | null>(null);
const allTracksScrollTop = ref(0);
const allTracksViewportHeight = ref(0);
const allTracksViewportWidth = ref(0);
const sortBarHeight = ref(0);
let allTracksResizeObserver: ResizeObserver | null = null;

function setAllTracksScrollEl(el: TemplateRefEl) {
  allTracksResizeObserver?.disconnect();
  allTracksResizeObserver = null;
  const scrollEl = el instanceof HTMLElement ? el : null;
  allTracksScrollEl.value = scrollEl;
  if (!scrollEl) return;
  allTracksViewportHeight.value = scrollEl.clientHeight;
  allTracksViewportWidth.value = scrollEl.clientWidth;
  allTracksResizeObserver = new ResizeObserver(() => {
    allTracksViewportHeight.value = scrollEl.clientHeight;
    allTracksViewportWidth.value = scrollEl.clientWidth;
  });
  allTracksResizeObserver.observe(scrollEl);
}

function setSortBarEl(el: TemplateRefEl) {
  sortBarHeight.value = el instanceof HTMLElement ? el.offsetHeight : 0;
}

function onAllTracksScroll() {
  if (allTracksScrollEl.value) allTracksScrollTop.value = allTracksScrollEl.value.scrollTop;
}

onUnmounted(() => allTracksResizeObserver?.disconnect());

const trackRowRange = computed(() => {
  const total = sortedFilteredTracks.value.length;
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
  sortedFilteredTracks.value.slice(trackRowRange.value.start, trackRowRange.value.end)
);

// status, bpm, added, actions, remove, plus one per visible metadata column.
const columnCount = computed(() => 5 + store.orderedVisibleColumns.length);

// While a column is being actively resized, its own growing width would
// otherwise feed back into the proportional share every other column gets
// from the leftover space, compounding into a runaway "acceleration" as the
// pointer moves. Freezing every column at its raw configured width for the
// duration of the drag avoids that feedback loop entirely; distribution
// resumes the instant the drag ends.
function frozenWidths<F extends string>(
  fields: F[],
  getWidth: (field: F) => number
): Record<F, number> {
  const result = {} as Record<F, number>;
  for (const field of fields) result[field] = getWidth(field);
  return result;
}

const MAIN_TABLE_FIXED_TOTAL =
  FIXED_COLUMN_WIDTH.status +
  FIXED_COLUMN_WIDTH.bpm +
  FIXED_COLUMN_WIDTH.added +
  FIXED_COLUMN_WIDTH.actions +
  FIXED_COLUMN_WIDTH.remove;

const PLAYLIST_DETAIL_FIXED_TOTAL =
  FIXED_COLUMN_WIDTH.playlistIdx +
  FIXED_COLUMN_WIDTH.playlistGrip +
  FIXED_COLUMN_WIDTH.bpm +
  FIXED_COLUMN_WIDTH.added +
  FIXED_COLUMN_WIDTH.actions +
  FIXED_COLUMN_WIDTH.remove;

function sumValues(widths: Record<string, number>): number {
  return Object.values(widths).reduce((sum, width) => sum + width, 0);
}

const mainMetadataWidths = computed(() =>
  resizingColumn.value !== null
    ? frozenWidths(store.orderedVisibleColumns, store.getColumnWidth)
    : distributeColumnWidths(
        allTracksViewportWidth.value,
        MAIN_TABLE_FIXED_TOTAL,
        store.orderedVisibleColumns,
        store.getColumnWidth
      )
);

const playlistDetailMetadataWidths = computed(() =>
  resizingColumn.value !== null
    ? frozenWidths(store.orderedVisibleColumns, store.getColumnWidth)
    : distributeColumnWidths(
        playlistDetailViewportWidth.value,
        PLAYLIST_DETAIL_FIXED_TOTAL,
        store.orderedVisibleColumns,
        store.getColumnWidth
      )
);

const playlistListWidths = computed(() =>
  resizingPlaylistListColumn.value !== null
    ? frozenWidths(playlistListColumnsState.order, getPlaylistListColumnWidth)
    : distributeColumnWidths(
        playlistsOverviewViewportWidth.value,
        FIXED_COLUMN_WIDTH.remove,
        playlistListColumnsState.order,
        getPlaylistListColumnWidth
      )
);

// table-layout: fixed with a table width of "100%" makes browsers stretch
// every column proportionally whenever the declared column widths don't sum
// to exactly that 100%, including during an active resize where the other
// columns are intentionally frozen below their normal share. Binding the
// table's own width to this same sum sidesteps that stretch heuristic
// entirely: there's never a gap for the browser to redistribute.
const mainTableWidth = computed(() => MAIN_TABLE_FIXED_TOTAL + sumValues(mainMetadataWidths.value));
const playlistDetailTableWidth = computed(
  () => PLAYLIST_DETAIL_FIXED_TOTAL + sumValues(playlistDetailMetadataWidths.value)
);
const playlistListTableWidth = computed(
  () => FIXED_COLUMN_WIDTH.remove + sumValues(playlistListWidths.value)
);

const activePlaylist = computed(
  () => store.playlists.find((p) => p.id === activePlaylistId.value) ?? null
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

const showDropLine = computed((): boolean => {
  if (playlistDragFromIdx.value === null || playlistDropIdx.value === null) return false;
  if (playlistDropIdx.value === playlistDragFromIdx.value) return false;
  if (playlistDropIdx.value === playlistDragFromIdx.value + 1) return false;
  return true;
});

function openBpmModal(id: string) {
  bpmModalTrackId.value = id;
}

function onBpmSubmit(bpm: number) {
  if (bpmModalTrackId.value) store.setBpm(bpmModalTrackId.value, bpm);
  bpmModalTrackId.value = null;
}

function displayName(filename: string): string {
  return filename.replace(/\.(mp3|wav|flac|aac|ogg|m4a|aiff?)$/i, '');
}

function loadToDeck(path: string, deckId: string) {
  window.dispatchEvent(new CustomEvent('bm:collection-drop', { detail: { deckId, path } }));
}

function removeFromActivePlaylist(path: string) {
  if (activePlaylistId.value) store.removeFromPlaylist(activePlaylistId.value, path);
}

function onAddToActivePlaylist(track: CollectionEntry) {
  if (!track.path || !activePlaylistId.value) return;
  store.addToPlaylist(activePlaylistId.value, track.path);
}

function confirmClear() {
  store.clearAll();
  pendingClear.value = false;
}

function confirmRemoveTrack() {
  if (pendingRemoveTrackId.value) store.removeTrack(pendingRemoveTrackId.value);
  pendingRemoveTrackId.value = null;
}

function confirmDeletePlaylist() {
  if (pendingDeletePlaylistId.value) {
    if (activePlaylistId.value === pendingDeletePlaylistId.value) {
      activePlaylistId.value = null;
    }
    store.deletePlaylist(pendingDeletePlaylistId.value);
  }
  pendingDeletePlaylistId.value = null;
}

function onTrackDblClick(track: CollectionEntry) {
  if (track.status !== 'ready' || !track.path) return;
  const target = decksStore.bestAvailableDeck(appModeStore.mode === 'edit');
  if (!target) return;
  loadToDeck(track.path, target);
}

function onTrackDblClickByPath(path: string) {
  const entry = store.tracks.find((t) => t.path === path);
  if (!entry || entry.status !== 'ready') return;
  const target = decksStore.bestAvailableDeck(appModeStore.mode === 'edit');
  if (!target) return;
  loadToDeck(path, target);
}

function openPlaylist(id: string) {
  activePlaylistId.value = id;
  showAddSection.value = false;
  addSectionSearch.value = '';
  renamingPlaylist.value = false;
}

async function onCreatePlaylist() {
  store.createPlaylist(`Playlist ${store.playlists.length + 1}`);
  const created = store.playlists[store.playlists.length - 1];
  activePlaylistId.value = created.id;
  showAddSection.value = false;
  renameValue.value = created.name;
  renamingPlaylist.value = true;
  await nextTick();
  renameInputEl.value?.select();
}

async function startRename() {
  const p = activePlaylist.value;
  if (!p) return;
  renameValue.value = p.name;
  renamingPlaylist.value = true;
  await nextTick();
  renameInputEl.value?.select();
}

function confirmRename() {
  const name = renameValue.value.trim();
  if (name && activePlaylistId.value) {
    store.renamePlaylist(activePlaylistId.value, name);
  }
  renamingPlaylist.value = false;
}

function cancelRename() {
  renamingPlaylist.value = false;
}

function onPlaylistTrackPointerDown(e: PointerEvent, fromIdx: number) {
  if (e.button !== 0) return;
  if ((e.target as HTMLElement).closest('button')) return;

  const playlist = activePlaylist.value;
  if (!playlist) return;

  playlistDragFromIdx.value = fromIdx;
  playlistDropIdx.value = fromIdx;

  function computeDropIdx(clientY: number): number {
    const el = playlistListEl.value;
    if (!el) return fromIdx;
    const items = el.querySelectorAll<HTMLElement>('.collection__playlist-track');
    for (let i = 0; i < items.length; i++) {
      const rect = items[i].getBoundingClientRect();
      if (clientY < rect.top + rect.height / 2) {
        playlistDropY.value = items[i].offsetTop;
        return i;
      }
    }
    const last = items[items.length - 1];
    playlistDropY.value = last ? last.offsetTop + last.offsetHeight : 0;
    return items.length;
  }

  function onMove(ev: PointerEvent) {
    playlistDropIdx.value = computeDropIdx(ev.clientY);
  }

  function resetDrag() {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    window.removeEventListener('pointercancel', onCancel);
    playlistDragFromIdx.value = null;
    playlistDropIdx.value = null;
  }

  function onUp(ev: PointerEvent) {
    const from = playlistDragFromIdx.value;
    const dropIdx = computeDropIdx(ev.clientY);
    resetDrag();
    if (from === null) return;
    if (dropIdx === from || dropIdx === from + 1) return;
    const to = dropIdx > from ? dropIdx - 1 : dropIdx;
    store.moveInPlaylist(playlist?.id ?? null, from, to);
  }

  function onCancel() {
    resetDrag();
  }

  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onUp);
  window.addEventListener('pointercancel', onCancel);
}

const AUDIO_EXT = /\.(mp3|wav|flac|aac|ogg|m4a|aiff?)$/i;

// OS file/folder drops come through Tauri's native drag-drop (HTML5 DnD can't see
// absolute paths in Tauri v2). Dropped audio files are added directly; dropped
// folders are scanned, same as the file/folder dialogs. Internal track drags
// (store.draggingPath) are pointer-based and never fire this event.
async function onFilesDropped(paths: string[]) {
  if (store.draggingPath) return;
  const audioFiles = paths.filter((p) => AUDIO_EXT.test(p));
  const folders = paths.filter((p) => !AUDIO_EXT.test(p));
  if (audioFiles.length > 0) store.addFilesFromPaths(audioFiles);
  if (folders.length > 0) {
    const scanned = await store.scanFolders(folders);
    if (scanned.length > 0) store.addFilesFromPaths(scanned);
  }
}

let unlistenDrop: UnlistenFn | null = null;
onMounted(async () => {
  unlistenDrop = await getCurrentWebview().onDragDropEvent(async (event) => {
    const payload = event.payload;
    if (payload.type === 'enter' || payload.type === 'over') {
      if (!store.draggingPath) isDragOver.value = true;
    } else if (payload.type === 'leave') {
      isDragOver.value = false;
    } else if (payload.type === 'drop') {
      isDragOver.value = false;
      await onFilesDropped(payload.paths);
    }
  });
});
onUnmounted(() => unlistenDrop?.());

// Movement below this threshold is treated as a click, not a drag start.
const DRAG_THRESHOLD = 5;

const DRAG_GHOST_SCALE = 0.6;

type DragGhost = { element: HTMLElement; halfWidth: number; halfHeight: number };

// Tracked at module level so a ghost orphaned by an earlier drag (e.g. the
// pointerup was lost because the window lost focus) can never pile up: each
// new drag removes any leftover ghost before creating its own.
let currentDragGhost: DragGhost | null = null;

function clearDragGhost() {
  currentDragGhost?.element.remove();
  currentDragGhost = null;
}

// A <tr> cloned on its own and appended to <body> loses the table's column
// model (no <colgroup>/<table> ancestor), so its cells would render squished
// instead of matching the real row. Wrapping the clone in a table that
// carries the same <colgroup> keeps the ghost's column widths identical.
// The colgroup must come from the still-attached original row: the clone is
// detached, so `closest` on it can never find an ancestor table.
function wrapRowClone(
  row: HTMLTableRowElement,
  originalRow: HTMLTableRowElement
): HTMLTableElement {
  const table = document.createElement('table');
  table.style.borderCollapse = 'collapse';
  table.style.tableLayout = 'fixed';
  const colgroup = originalRow.closest('table')?.querySelector('colgroup');
  if (colgroup) table.appendChild(colgroup.cloneNode(true));
  const tbody = document.createElement('tbody');
  tbody.appendChild(row);
  table.appendChild(tbody);
  return table;
}

// transform-origin defaults to the element's own center, so scaling never
// shifts that center: left/top only need the unscaled half-size offset, and
// that offset never changes again for the rest of the drag.
function createDragGhost(source: HTMLElement, clientX: number, clientY: number): DragGhost {
  clearDragGhost();
  const rect = source.getBoundingClientRect();
  const clone = source.cloneNode(true) as HTMLElement;
  clone.querySelectorAll('button').forEach((button) => button.remove());
  const element =
    clone instanceof HTMLTableRowElement && source instanceof HTMLTableRowElement
      ? wrapRowClone(clone, source)
      : clone;
  element.classList.add('collection__drag-ghost');
  element.style.width = `${rect.width}px`;
  element.style.height = `${rect.height}px`;
  const halfWidth = rect.width / 2;
  const halfHeight = rect.height / 2;
  element.style.left = `${clientX - halfWidth}px`;
  element.style.top = `${clientY - halfHeight}px`;
  document.body.appendChild(element);
  // Only `transform` animates here, never left/top, so the brief shrink-in
  // never delays cursor tracking.
  requestAnimationFrame(() => {
    element.style.transition = 'transform 100ms ease';
    element.style.transform = `scale(${DRAG_GHOST_SCALE})`;
  });
  const ghost: DragGhost = { element, halfWidth, halfHeight };
  currentDragGhost = ghost;
  return ghost;
}

function moveDragGhost(ghost: DragGhost, clientX: number, clientY: number) {
  ghost.element.style.left = `${clientX - ghost.halfWidth}px`;
  ghost.element.style.top = `${clientY - ghost.halfHeight}px`;
}

function resolveDeckIdAtPoint(clientX: number, clientY: number): string | undefined {
  const el = document.elementFromPoint(clientX, clientY);
  const deckEl = el?.closest('[data-deck-id]') as HTMLElement | null;
  return deckEl?.dataset.deckId;
}

function onItemPointerDown(event: PointerEvent, track: CollectionEntry) {
  if (event.button !== 0 || track.status !== 'ready' || !track.path) return;
  if ((event.target as HTMLElement).closest('button')) return;

  const startX = event.clientX;
  const startY = event.clientY;
  const path = track.path;
  const itemEl = event.currentTarget as HTMLElement;
  let active = false;
  let dragGhost: DragGhost | null = null;

  function onMove(ev: PointerEvent) {
    if (!active) {
      if (
        Math.abs(ev.clientX - startX) < DRAG_THRESHOLD &&
        Math.abs(ev.clientY - startY) < DRAG_THRESHOLD
      )
        return;
      active = true;
      store.startDrag(path);
      document.body.style.cursor = 'grabbing';
      // The search input can be left focused from an earlier click; without
      // blurring it here, keyboard shortcuts typed during/after the drag go
      // into the search box instead of controlling decks.
      const focused = document.activeElement;
      if (focused instanceof HTMLInputElement) focused.blur();
      dragGhost = createDragGhost(itemEl, ev.clientX, ev.clientY);
      return;
    }
    if (dragGhost) moveDragGhost(dragGhost, ev.clientX, ev.clientY);
  }

  function finishDrag(): boolean {
    const wasActive = active;
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    window.removeEventListener('pointercancel', onCancel);
    window.removeEventListener('blur', onCancel);
    clearDragGhost();
    dragGhost = null;
    if (wasActive) {
      document.body.style.cursor = '';
      store.endDrag();
    }
    return wasActive;
  }

  function onUp(ev: PointerEvent) {
    if (!finishDrag()) return;
    const deckId = resolveDeckIdAtPoint(ev.clientX, ev.clientY);
    if (deckId) {
      window.dispatchEvent(new CustomEvent('bm:collection-drop', { detail: { deckId, path } }));
    }
  }

  function onCancel() {
    finishDrag();
  }

  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onUp);
  window.addEventListener('pointercancel', onCancel);
  // If the window loses focus mid-drag (alt-tab, native dialog), no further
  // pointer events arrive at all, so this is the only way to clean up.
  window.addEventListener('blur', onCancel);
}

async function openFileDialog() {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const result = await open({
    multiple: true,
    filters: [
      { name: 'Audio', extensions: ['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a', 'aif', 'aiff'] }
    ]
  });
  if (result) {
    const paths = Array.isArray(result) ? result : [result];
    store.addFilesFromPaths(paths);
  }
}

async function openFolderDialog() {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const result = await open({ directory: true, multiple: true });
  if (!result) return;
  const folders = Array.isArray(result) ? result : [result];
  const paths = await store.scanFolders(folders);
  if (paths.length > 0) store.addFilesFromPaths(paths);
}
</script>

<style scoped>
.collection {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--color-bg);
  border-top: 1px solid var(--color-border);
  transition: outline 0.1s;
}

.collection--drag-over {
  outline: 2px dashed var(--color-muted);
  outline-offset: -4px;
}

.collection__header {
  display: flex;
  align-items: center;
  gap: 0.8em;
  padding: 0 4px;
  height: 29px;
  flex-shrink: 0;
  border-bottom: 1px solid var(--color-border);
}

.collection__tabs {
  display: flex;
  gap: 0;
  flex-shrink: 0;
}

.collection__tab {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.7em;
  letter-spacing: 0.02em;
  height: 22px;
  padding: 0 0.6em;
  display: flex;
  align-items: center;
  cursor: pointer;
  text-transform: uppercase;
}

.collection__tab:first-child {
  border-radius: 3px 0 0 3px;
}

.collection__tab:last-child {
  border-radius: 0 3px 3px 0;
  margin-left: -1px;
}

.collection__tab:hover {
  color: var(--color-text);
}

.collection__tab--active {
  z-index: 1;
  border-color: #555;
  color: var(--color-text);
  background: var(--color-surface);
}

.collection__count {
  font-size: 0.8em;
  color: var(--color-muted);
  opacity: 0.6;
}

.collection__header-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-left: auto;
}

.collection__header-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.75em;
  letter-spacing: 0.02em;
  height: 22px;
  padding: 0 0.7em;
  display: flex;
  align-items: center;
  text-transform: uppercase;
  border-radius: 3px;
  cursor: pointer;
  margin-left: auto;
}

.collection__header-actions .collection__header-btn {
  margin-left: 0;
}

.collection__header-btn:hover {
  border-color: #555;
  color: var(--color-text);
}

.collection__header-btn--muted {
  opacity: 0.5;
}

.collection__header-btn--muted:hover {
  opacity: 1;
}

.collection__playlist-title {
  margin-left: auto;
  min-width: 0;
  font-size: 0.78em;
  color: var(--color-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
  border: 1px solid transparent;
  padding: 0.2em 0.4em;
  border-radius: 3px;
}

.collection__playlist-title:hover {
  color: #fff;
}

.collection__playlist-rename {
  margin-left: auto;
  min-width: 0;
  width: 14em;
  background: transparent;
  border: 1px solid #555;
  color: var(--color-text);
  font-family: var(--font);
  font-size: 0.78em;
  padding: 0.2em 0.4em;
  border-radius: 3px;
  outline: none;
}

.collection__body {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
}

.collection__body--playlist {
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.collection__body--playlist .collection__list {
  position: relative;
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-gutter: stable;
}

.collection__empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  font-size: 0.78em;
  color: var(--color-muted);
  opacity: 0.5;
  letter-spacing: 0.02em;
}

.collection__list {
  display: flex;
  flex-direction: column;
}

.collection__item {
  display: flex;
  align-items: center;
  gap: 0.6em;
  padding: 0 4px;
  height: 32px;
  border-bottom: 1px solid var(--color-border);
  cursor: default;
  transition: background 0.1s;
  font-size: 0.8em;
}

.collection__item:hover {
  background: var(--color-surface);
}

.collection__item--ready {
  cursor: grab;
}

.collection__item--ready:active {
  cursor: grabbing;
}

.collection__item--playlist {
  cursor: pointer;
}

.collection__item-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--color-text);
}

.collection__item-bpm {
  color: var(--color-muted);
  font-size: 0.9em;
  white-space: nowrap;
  letter-spacing: 0.02em;
}

.collection__td--added {
  color: var(--color-muted);
  font-size: 0.85em;
  letter-spacing: 0.02em;
  white-space: nowrap;
}

.collection__item-tag {
  font-size: 0.85em;
  color: var(--color-muted);
  opacity: 0.6;
  white-space: nowrap;
}

.collection__item-tag--error {
  color: var(--color-danger);
  opacity: 1;
}

.collection__item-tag--missing {
  opacity: 0.4;
}

.collection__item--missing {
  opacity: 0.45;
  cursor: default;
}

.collection__item--analyzing .collection__item-name {
  opacity: 0.5;
}

.collection__item--played .collection__item-name {
  color: var(--color-muted);
}

.collection__item-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.85em;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  padding: 0.15em 0.5em;
  border-radius: 3px;
  cursor: pointer;
  white-space: nowrap;
}

.collection__item-btn:hover {
  border-color: #555;
  color: var(--color-text);
}

.collection__item-remove {
  background: transparent;
  border: none;
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.9em;
  width: 1.4em;
  height: 1.4em;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 3px;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.1s;
  flex-shrink: 0;
  padding: 0;
}

.collection__item:hover .collection__item-remove,
.collection__row:hover .collection__item-remove {
  opacity: 0.5;
}

.collection__item-remove:hover {
  opacity: 1 !important;
  color: var(--color-text);
}

.collection__table {
  table-layout: fixed;
  border-collapse: collapse;
  font-size: 0.8em;
}

.collection__head-row {
  background: var(--color-surface);
}

.collection__th {
  border-bottom: 1px solid var(--color-border);
  padding: 0.35em 4px;
  font-size: 0.9em;
  letter-spacing: 0.02em;
  color: var(--color-muted);
  font-weight: normal;
  text-align: left;
  text-transform: uppercase;
}

.collection__th--actions {
  text-align: right;
}

.collection__th--meta {
  position: relative;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  cursor: grab;
  user-select: none;
}

.collection__th--meta:active {
  cursor: grabbing;
}

.collection__th--dragging {
  opacity: 0.4;
}

.collection__th--drop-target {
  background: var(--color-surface);
  box-shadow: inset 0 0 0 1px var(--color-text);
}

.collection__th-label {
  pointer-events: none;
}

.collection__col-resizer {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  width: 6px;
  cursor: col-resize;
  transform: translateX(50%);
}

.collection__sort-btn {
  width: 100%;
  background: transparent;
  border: none;
  color: inherit;
  font-family: var(--font);
  font-size: 1em;
  letter-spacing: inherit;
  padding: 0;
  cursor: pointer;
  white-space: nowrap;
  text-align: inherit;
}

.collection__sort-btn:hover {
  color: var(--color-text);
}

.collection__row {
  height: 32px;
  cursor: default;
  transition: background 0.1s;
}

.collection__row:hover {
  background: var(--color-surface);
}

.collection__row.collection__item--ready {
  cursor: grab;
}

.collection__row.collection__item--ready:active {
  cursor: grabbing;
}

.collection__td {
  border-bottom: 1px solid var(--color-border);
  padding: 0 4px;
  overflow: hidden;
}

.collection__td--bpm,
.collection__td--added {
  text-align: left;
}

.collection__td--meta {
  cursor: text;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.collection__meta-value {
  color: var(--color-muted);
}

.collection__meta-input {
  width: 100%;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  color: var(--color-text);
  font-family: var(--font);
  font-size: inherit;
  padding: 1px 3px;
  border-radius: 2px;
}

.collection__item-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 0.6em;
}

.collection__td--remove {
  text-align: center;
}

.collection__playlist-track {
  cursor: grab;
}

.collection__playlist-track:active {
  cursor: grabbing;
}

.collection__playlist-track--dragging {
  opacity: 0.35;
}

.collection__playlist-num {
  color: var(--color-muted);
  opacity: 0.45;
  font-size: 0.75em;
  flex-shrink: 0;
  min-width: 1.6em;
  text-align: right;
  user-select: none;
}

.collection__playlist-grip {
  color: var(--color-muted);
  font-size: 0.9em;
  flex-shrink: 0;
  user-select: none;
}

.collection__drop-line {
  position: absolute;
  left: 0;
  right: 0;
  height: 2px;
  background: var(--color-text);
  opacity: 0.6;
  pointer-events: none;
}

.collection__add-section {
  border-top: 1px solid var(--color-border);
  flex-shrink: 0;
}

.collection__add-toggle {
  width: 100%;
  background: transparent;
  border: none;
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.75em;
  letter-spacing: 0.02em;
  padding: 0.6em 1em;
  cursor: pointer;
  text-align: left;
  display: block;
  text-transform: uppercase;
}

.collection__add-toggle:hover {
  color: var(--color-text);
}

.collection__add-body {
  border-top: 1px solid var(--color-border);
  max-height: 200px;
  overflow-y: auto;
}
</style>

<style>
.collection__drag-ghost {
  position: fixed;
  z-index: 2000;
  pointer-events: none;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.6);
}

.context-menu__backdrop {
  position: fixed;
  inset: 0;
  z-index: 999;
}

.context-menu {
  position: fixed;
  z-index: 1000;
  background: #1a1a1a;
  border: 1px solid #333;
  border-radius: 4px;
  padding: 4px 0;
  min-width: 160px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.6);
  font-family: var(--font);
}

.context-menu__item {
  display: block;
  width: 100%;
  padding: 6px 14px;
  background: none;
  border: none;
  color: var(--color-text);
  font-family: var(--font);
  font-size: 0.75rem;
  letter-spacing: 0.02em;
  text-align: left;
  cursor: pointer;
}

.context-menu__item:hover {
  background: #2a2a2a;
  color: #fff;
}

.context-menu__separator {
  height: 1px;
  background: #333;
  margin: 4px 0;
}

.context-menu__title {
  padding: 4px 14px 6px;
  font-size: 0.65rem;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--color-muted);
}

.context-menu__checkbox {
  display: inline-block;
  width: 1.2em;
}

.context-menu__item--sub {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: space-between;
  cursor: default;
}

.context-menu__item--disabled {
  display: flex;
  align-items: center;
  justify-content: space-between;
  cursor: not-allowed;
  opacity: 0.45;
}

.context-menu__item--disabled:hover {
  background: none;
  color: var(--color-text);
}

.context-menu__item-hint {
  font-size: 0.85em;
  margin-left: 12px;
  white-space: nowrap;
}

.context-menu__arrow {
  font-size: 0.6em;
  opacity: 0.5;
  margin-left: 12px;
}

.context-menu__submenu {
  display: none;
  position: absolute;
  left: 100%;
  top: -4px;
  background: #1a1a1a;
  border: 1px solid #333;
  border-radius: 4px;
  padding: 4px 0;
  min-width: 160px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.6);
  z-index: 1001;
}

.context-menu__item--sub:hover .context-menu__submenu {
  display: block;
}

.context-menu__submenu--flip {
  top: auto;
  bottom: -4px;
}
</style>
