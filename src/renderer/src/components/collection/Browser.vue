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
      <div v-else class="collection__list">
        <div :ref="setSortBarEl" class="collection__sort-bar">
          <button
            tabindex="-1"
            class="collection__sort-btn collection__sort-btn--title"
            @click="toggleSort('title')"
          >
            {{ $t('browser.colTitle')
            }}{{ sortField === 'title' ? (sortDir === 'asc' ? ' ▲' : ' ▼') : '' }}
          </button>
          <button tabindex="-1" class="collection__sort-btn" @click="toggleSort('bpm')">
            {{ $t('browser.colBpm')
            }}{{ sortField === 'bpm' ? (sortDir === 'asc' ? ' ▲' : ' ▼') : '' }}
          </button>
          <button tabindex="-1" class="collection__sort-btn" @click="toggleSort('added')">
            {{ $t('browser.colAdded')
            }}{{ sortField === 'added' ? (sortDir === 'asc' ? ' ▲' : ' ▼') : '' }}
          </button>
        </div>
        <div
          v-if="trackRowRange.topSpacerHeight > 0"
          :style="{ height: `${trackRowRange.topSpacerHeight}px` }"
        />
        <div
          v-for="track in visibleTracks"
          :key="track.id"
          class="collection__item"
          :class="[
            `collection__item--${track.status}`,
            { 'collection__item--played': track.path && mixerStore.playedPaths.has(track.path) }
          ]"
          @pointerdown="onItemPointerDown($event, track)"
          @dblclick="onTrackDblClick(track)"
          @contextmenu.prevent="openContextMenu($event, track.id)"
        >
          <span class="collection__item-name" :title="track.title ?? displayName(track.name)">{{
            track.title ?? displayName(track.name)
          }}</span>
          <span v-if="store.getBpm(track) !== null" class="collection__item-bpm">
            {{ store.getBpm(track)?.toFixed(1) }} BPM
          </span>
          <span v-else-if="track.status === 'analyzing'" class="collection__item-tag">
            {{ $t('browser.detecting') }}
          </span>
          <span
            v-else-if="track.status === 'error'"
            class="collection__item-tag collection__item-tag--error"
            >{{ $t('browser.statusError') }}</span
          >
          <span
            v-if="track.status === 'missing'"
            class="collection__item-tag collection__item-tag--missing"
            >{{ $t('browser.statusMissing') }}</span
          >
          <Buttons v-if="track.status === 'ready' && track.path" :path="track.path ?? ''" />
          <button
            v-if="track.status === 'idle'"
            class="collection__item-btn"
            tabindex="-1"
            @click.stop="store.analyzeTrack(track.id)"
          >
            {{ $t('browser.analyze') }}
          </button>
          <button
            v-if="track.status === 'error'"
            class="collection__item-btn"
            tabindex="-1"
            @click.stop="openBpmModal(track.id)"
          >
            {{ $t('browser.setBpm') }}
          </button>
          <button
            v-if="track.status === 'missing'"
            class="collection__item-btn"
            tabindex="-1"
            @click.stop="store.locateMissingTracks()"
          >
            {{ $t('browser.locate') }}
          </button>
          <button
            class="collection__item-remove"
            tabindex="-1"
            @click.stop="pendingRemoveTrackId = track.id"
          >
            ✕
          </button>
        </div>
        <div
          v-if="trackRowRange.bottomSpacerHeight > 0"
          :style="{ height: `${trackRowRange.bottomSpacerHeight}px` }"
        />
      </div>
    </div>

    <div v-else-if="activePlaylistId === null" class="collection__body">
      <div v-if="store.playlists.length === 0" class="collection__empty">
        {{ $t('browser.noPlaylists') }}
      </div>
      <div v-else class="collection__list">
        <div
          v-for="playlist in store.playlists"
          :key="playlist.id"
          class="collection__item collection__item--playlist"
          @click="openPlaylist(playlist.id)"
        >
          <span class="collection__item-name">{{ playlist.name }}</span>
          <span class="collection__item-bpm">{{
            $t('browser.trackCount', playlist.paths.length)
          }}</span>
          <button
            class="collection__item-remove"
            tabindex="-1"
            @click.stop="pendingDeletePlaylistId = playlist.id"
          >
            ✕
          </button>
        </div>
      </div>
    </div>

    <div v-else class="collection__body collection__body--playlist">
      <div ref="playlistListEl" class="collection__list">
        <div v-if="playlistItems.length === 0" class="collection__empty" style="height: 60px">
          {{ $t('browser.emptyPlaylist') }}
        </div>
        <div
          v-for="(item, idx) in playlistItems"
          :key="item.path"
          class="collection__item collection__playlist-track"
          :class="{
            'collection__playlist-track--dragging': playlistDragFromIdx === idx,
            'collection__item--played': mixerStore.playedPaths.has(item.path)
          }"
          @pointerdown="onPlaylistTrackPointerDown($event, idx)"
          @dblclick="onTrackDblClickByPath(item.path)"
          @contextmenu.prevent="item.entry && openContextMenu($event, item.entry.id)"
        >
          <span class="collection__playlist-num">{{ idx + 1 }}</span>
          <span class="collection__playlist-grip">⠿</span>
          <span class="collection__item-name" :title="item.label">{{ item.label }}</span>
          <span v-if="item.bpm !== null" class="collection__item-bpm"
            >{{ item.bpm.toFixed(1) }} BPM</span
          >
          <Buttons
            :path="item.path"
            :disabled="item.entry === null || item.entry.status !== 'ready'"
          />
          <button
            class="collection__item-remove"
            tabindex="-1"
            @click.stop="removeFromActivePlaylist(item.path)"
          >
            ✕
          </button>
        </div>
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
            <span class="collection__item-name" :title="track.title ?? displayName(track.name)">{{
              track.title ?? displayName(track.name)
            }}</span>
            <span v-if="store.getBpm(track) !== null" class="collection__item-bpm">
              {{ store.getBpm(track)?.toFixed(1) }} BPM
            </span>
            <button
              tabindex="-1"
              class="collection__item-btn"
              @click="
                track.path && activePlaylistId && store.addToPlaylist(activePlaylistId, track.path)
              "
            >
              +
            </button>
          </div>
        </div>
      </div>
    </div>

    <BpmModal
      :open="bpmModalTrackId !== null"
      :current-bpm="null"
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
  </div>
</template>

<script setup lang="ts">
import { ref, computed, nextTick, onMounted, onUnmounted, type ComponentPublicInstance } from 'vue';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { useCollectionStore } from '@renderer/stores/collection';
import { useDecksStore } from '@renderer/stores/decks';
import { useAppModeStore } from '@renderer/stores/appMode';
import { useMixerStore } from '@renderer/stores/mixer';
import type { CollectionEntry } from '@renderer/stores/collection';
import BpmModal from '@renderer/components/modals/BpmModal.vue';
import ConfirmModal from '@renderer/components/modals/ConfirmModal.vue';
import Search from '@renderer/components/collection/Search.vue';
import Buttons from '@renderer/components/collection/Buttons.vue';

const store = useCollectionStore();
const decksStore = useDecksStore();
const appModeStore = useAppModeStore();
const mixerStore = useMixerStore();

const isDragOver = ref(false);
const pendingClear = ref(false);
const bpmModalTrackId = ref<string | null>(null);
const searchQuery = ref('');

const tab = ref<'all' | 'playlists'>('all');
const activePlaylistId = ref<string | null>(null);
const renamingPlaylist = ref(false);
const renameValue = ref('');
const renameInputEl = ref<HTMLInputElement | null>(null);
const showAddSection = ref(false);
const addSectionSearch = ref('');

const playlistListEl = ref<HTMLElement | null>(null);
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

function onContextMenuAddToPlaylist(playlistId: string) {
  const track = store.tracks.find((t) => t.id === contextMenu.value?.trackId);
  if (track?.path) store.addToPlaylist(playlistId, track.path);
  closeContextMenu();
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
  if (sortField.value === 'added') {
    return sortDir.value === 'asc' ? [...tracks] : [...tracks].reverse();
  }
  return [...tracks].sort((a, b) => {
    let aVal: string | number | null;
    let bVal: string | number | null;
    if (sortField.value === 'title') {
      aVal = (a.title ?? displayName(a.name)).toLowerCase();
      bVal = (b.title ?? displayName(b.name)).toLowerCase();
    } else {
      aVal = store.getBpm(a);
      bVal = store.getBpm(b);
    }
    if (aVal === null && bVal === null) return 0;
    if (aVal === null) return 1;
    if (bVal === null) return -1;
    const cmp = aVal < bVal ? -1 : aVal > bVal ? 1 : 0;
    return sortDir.value === 'asc' ? cmp : -cmp;
  });
});

// Large collections (hundreds of tracks) made every row a permanent DOM node,
// so resizing the window forced a full flex/text-ellipsis layout pass over
// all of them every frame, even the ones scrolled out of view. Only rows
// within (or near) the visible scroll area are mounted; the rest are
// represented by two spacer divs sized to the height they'd otherwise take up.
const TRACK_ROW_HEIGHT = 32; // must match .collection__item height in <style>
const TRACK_ROW_BUFFER = 6;

type TemplateRefEl = Element | ComponentPublicInstance | null;

const allTracksScrollEl = ref<HTMLElement | null>(null);
const allTracksScrollTop = ref(0);
const allTracksViewportHeight = ref(0);
const sortBarHeight = ref(0);
let allTracksResizeObserver: ResizeObserver | null = null;

function setAllTracksScrollEl(el: TemplateRefEl) {
  allTracksResizeObserver?.disconnect();
  allTracksResizeObserver = null;
  const scrollEl = el instanceof HTMLElement ? el : null;
  allTracksScrollEl.value = scrollEl;
  if (!scrollEl) return;
  allTracksViewportHeight.value = scrollEl.clientHeight;
  allTracksResizeObserver = new ResizeObserver(() => {
    allTracksViewportHeight.value = scrollEl.clientHeight;
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

const activePlaylist = computed(
  () => store.playlists.find((p) => p.id === activePlaylistId.value) ?? null
);

type PlaylistItem = {
  path: string;
  entry: CollectionEntry | null;
  label: string;
  bpm: number | null;
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
    return { path, entry, label, bpm };
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

// transform-origin defaults to the element's own center, so scaling never
// shifts that center: left/top only need the unscaled half-size offset, and
// that offset never changes again for the rest of the drag.
function createDragGhost(source: HTMLElement, clientX: number, clientY: number): DragGhost {
  clearDragGhost();
  const rect = source.getBoundingClientRect();
  const element = source.cloneNode(true) as HTMLElement;
  element.classList.add('collection__drag-ghost');
  element.querySelectorAll('button').forEach((button) => button.remove());
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
  padding: 0 1em;
  height: 32px;
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
  letter-spacing: 0.12em;
  padding: 0.2em 0.6em;
  cursor: pointer;
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

.collection__header-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.75em;
  letter-spacing: 0.12em;
  padding: 0.25em 0.7em;
  border-radius: 3px;
  cursor: pointer;
  margin-left: auto;
}

.collection__header-btn + .collection__header-btn {
  margin-left: 0;
}

.collection__header-btn:hover {
  border-color: #555;
  color: var(--color-text);
}

.collection__header-btn--muted {
  margin-left: 0;
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
  letter-spacing: 0.05em;
}

.collection__list {
  display: flex;
  flex-direction: column;
}

.collection__item {
  display: flex;
  align-items: center;
  gap: 0.6em;
  padding: 0 1em;
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
  letter-spacing: 0.05em;
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
  letter-spacing: 0.1em;
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
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 3px;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.1s;
  flex-shrink: 0;
  padding: 0;
}

.collection__item:hover .collection__item-remove {
  opacity: 0.5;
}

.collection__item-remove:hover {
  opacity: 1 !important;
  color: var(--color-text);
}

.collection__sort-bar {
  display: flex;
  border-bottom: 1px solid var(--color-border);
  background: var(--color-surface);
  flex-shrink: 0;
}

.collection__sort-btn {
  background: transparent;
  border: none;
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.72em;
  letter-spacing: 0.12em;
  padding: 0.35em 0.8em;
  cursor: pointer;
  white-space: nowrap;
}

.collection__sort-btn:hover {
  color: var(--color-text);
}

.collection__sort-btn--title {
  flex: 1;
  text-align: left;
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
  letter-spacing: 0.12em;
  padding: 0.6em 1em;
  cursor: pointer;
  text-align: left;
  display: block;
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
  letter-spacing: 0.05em;
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
  cursor: default;
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
