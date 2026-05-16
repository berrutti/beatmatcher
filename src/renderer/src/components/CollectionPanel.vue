<template>
  <div
    class="collection"
    :class="{ 'collection--drag-over': isDragOver }"
    @dragover="onDragOver"
    @dragleave="onDragLeave"
    @drop="onDrop"
  >
    <div class="collection__header">
      <div class="collection__tabs">
        <button
          class="collection__tab"
          :class="{ 'collection__tab--active': tab === 'all' }"
          @click="tab = 'all'"
        >
          ALL
        </button>
        <button
          class="collection__tab"
          :class="{ 'collection__tab--active': tab === 'playlists' }"
          @click="tab = 'playlists'"
        >
          PLAYLISTS
        </button>
      </div>

      <template v-if="tab === 'all'">
        <span v-if="store.tracks.length > 0" class="collection__count"
          >{{ filteredTracks.length }}/{{ store.tracks.length }}</span
        >
        <div class="collection__search-wrap">
          <input
            v-model="searchQuery"
            class="collection__search"
            type="text"
            placeholder="Search"
            spellcheck="false"
            @pointerdown="onSearchPointerDown"
            @keydown.esc="searchQuery = ''"
          />
          <button
            v-if="searchQuery"
            class="collection__search-clear"
            tabindex="-1"
            @click="searchQuery = ''"
          >
            ✕
          </button>
        </div>
        <button v-if="store.hasPending" class="collection__header-btn" @click="store.analyzeAll()">
          ANALYZE ALL
        </button>
        <button class="collection__header-btn" @click="openFileDialog">ADD FILES</button>
        <button class="collection__header-btn" @click="openFolderDialog">ADD FOLDER</button>
        <button
          v-if="store.tracks.length > 0"
          class="collection__header-btn collection__header-btn--muted"
          @click="store.clearAll()"
        >
          CLEAR
        </button>
      </template>

      <template v-else-if="activePlaylistId === null">
        <button class="collection__header-btn" @click="onCreatePlaylist">NEW PLAYLIST</button>
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
          class="collection__header-btn"
          style="margin-left: 0"
          @click="activePlaylistId = null"
        >
          BACK
        </button>
      </template>
    </div>

    <div
      v-if="tab === 'all'"
      class="collection__body"
      :style="store.draggingPath ? { overflowY: 'hidden' } : {}"
    >
      <div v-if="store.tracks.length === 0" class="collection__empty">
        Drop audio files or folders here, or use ADD FILES / ADD FOLDER
      </div>
      <div v-else-if="sortedFilteredTracks.length === 0" class="collection__empty">no results</div>
      <div v-else class="collection__list">
        <div class="collection__sort-bar">
          <button
            class="collection__sort-btn collection__sort-btn--title"
            @click="toggleSort('title')"
          >
            TITLE{{ sortField === 'title' ? (sortDir === 'asc' ? ' ▲' : ' ▼') : '' }}
          </button>
          <button class="collection__sort-btn" @click="toggleSort('bpm')">
            BPM{{ sortField === 'bpm' ? (sortDir === 'asc' ? ' ▲' : ' ▼') : '' }}
          </button>
          <button class="collection__sort-btn" @click="toggleSort('added')">
            ADDED{{ sortField === 'added' ? (sortDir === 'asc' ? ' ▲' : ' ▼') : '' }}
          </button>
        </div>
        <div
          v-for="track in sortedFilteredTracks"
          :key="track.id"
          class="collection__item"
          :class="`collection__item--${track.status}`"
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
            detecting...
          </span>
          <span
            v-else-if="track.status === 'error'"
            class="collection__item-tag collection__item-tag--error"
            >error</span
          >
          <span
            v-if="track.status === 'missing'"
            class="collection__item-tag collection__item-tag--missing"
            >missing</span
          >
          <div v-if="track.status === 'ready' && track.path" class="collection__item-decks">
            <template v-if="decksStore.editMode">
              <button
                class="collection__deck-btn"
                :class="{ 'collection__deck-btn--loaded': deckHasTrack('E', track.path) }"
                :style="{ '--btn-color': decksStore.deckE.accent }"
                :disabled="deckHasTrack('E', track.path)"
                title="Click to send to Edit"
                @click.stop="loadToDeck(track.path, 'E')"
              >
                Edit
              </button>
            </template>
            <template v-else>
              <button
                v-for="deckId in DECKS_DISPOSITION"
                :key="deckId"
                class="collection__deck-btn"
                :class="{ 'collection__deck-btn--loaded': deckHasTrack(deckId, track.path) }"
                :style="{ '--btn-color': decksStore.decks[deckId].accent }"
                :disabled="deckHasTrack(deckId, track.path)"
                :title="`Click to send to Deck ${deckId}`"
                @click.stop="loadToDeck(track.path, deckId)"
              >
                Deck {{ deckId }}
              </button>
            </template>
          </div>
          <button
            v-if="track.status === 'idle'"
            class="collection__item-btn"
            @click.stop="store.analyzeTrack(track.id)"
          >
            ANALYZE
          </button>
          <button
            v-if="track.status === 'error'"
            class="collection__item-btn"
            @click.stop="openBpmModal(track.id)"
          >
            SET BPM
          </button>
          <button class="collection__item-remove" @click.stop="pendingRemoveTrackId = track.id">
            ✕
          </button>
        </div>
      </div>
    </div>

    <div v-else-if="activePlaylistId === null" class="collection__body">
      <div v-if="store.playlists.length === 0" class="collection__empty">
        No playlists yet. Click NEW PLAYLIST to get started.
      </div>
      <div v-else class="collection__list">
        <div
          v-for="playlist in store.playlists"
          :key="playlist.id"
          class="collection__item collection__item--playlist"
          @click="openPlaylist(playlist.id)"
        >
          <span class="collection__item-name">{{ playlist.name }}</span>
          <span class="collection__item-bpm"
            >{{ playlist.paths.length }} track{{ playlist.paths.length !== 1 ? 's' : '' }}</span
          >
          <button
            class="collection__item-remove"
            @click.stop="pendingDeletePlaylistId = playlist.id"
          >
            ✕
          </button>
        </div>
      </div>
    </div>

    <div v-else class="collection__body collection__body--playlist">
      <div
        ref="playlistListEl"
        class="collection__list"
        :style="playlistDragFromIdx !== null ? { overflowY: 'hidden' } : {}"
      >
        <div v-if="playlistItems.length === 0" class="collection__empty" style="height: 60px">
          Empty. Add tracks from the section below.
        </div>
        <template v-for="(item, idx) in playlistItems" :key="item.path">
          <div v-if="showDropBefore(idx)" class="collection__drop-line" />
          <div
            class="collection__item collection__playlist-track"
            :class="{ 'collection__playlist-track--dragging': playlistDragFromIdx === idx }"
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
            <div class="collection__item-decks">
              <template v-if="decksStore.editMode">
                <button
                  class="collection__deck-btn"
                  :class="{ 'collection__deck-btn--loaded': deckHasTrack('E', item.path) }"
                  :style="{ '--btn-color': decksStore.deckE.accent }"
                  :disabled="
                    item.entry === null ||
                    item.entry.status !== 'ready' ||
                    deckHasTrack('E', item.path)
                  "
                  title="Click to send to Edit"
                  @click.stop="loadToDeck(item.path, 'E')"
                >
                  Edit
                </button>
              </template>
              <template v-else>
                <button
                  v-for="deckId in DECKS_DISPOSITION"
                  :key="deckId"
                  class="collection__deck-btn"
                  :class="{ 'collection__deck-btn--loaded': deckHasTrack(deckId, item.path) }"
                  :style="{ '--btn-color': decksStore.decks[deckId].accent }"
                  :disabled="
                    item.entry === null ||
                    item.entry.status !== 'ready' ||
                    deckHasTrack(deckId, item.path)
                  "
                  :title="`Click to send to Deck ${deckId}`"
                  @click.stop="loadToDeck(item.path, deckId)"
                >
                  Deck {{ deckId }}
                </button>
              </template>
            </div>
            <button
              class="collection__item-remove"
              @click.stop="removeFromActivePlaylist(item.path)"
            >
              ✕
            </button>
          </div>
        </template>
        <div v-if="showDropAfter" class="collection__drop-line" />
      </div>

      <div class="collection__add-section">
        <button class="collection__add-toggle" @click="showAddSection = !showAddSection">
          {{ showAddSection ? '▾' : '▸' }} ADD TRACKS
        </button>
        <div v-if="showAddSection" class="collection__add-body">
          <div class="collection__add-search-wrap">
            <input
              v-model="addSectionSearch"
              class="collection__search"
              type="text"
              placeholder="search"
              spellcheck="false"
              @pointerdown="onSearchPointerDown"
              @keydown.esc="addSectionSearch = ''"
            />
            <button
              v-if="addSectionSearch"
              class="collection__search-clear"
              @click="addSectionSearch = ''"
            >
              ✕
            </button>
          </div>
          <div v-if="addableTracks.length === 0" class="collection__empty" style="height: 40px">
            {{
              store.tracks.filter((t) => t.status === 'ready').length === 0
                ? 'No analyzed tracks in collection'
                : 'All tracks already in playlist'
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
      :open="pendingDeletePlaylistId !== null"
      :title="`Delete '${pendingDeletePlaylistName}'?`"
      body="This cannot be undone. Tracks in your collection are not affected."
      confirm-label="Delete"
      @confirm="confirmDeletePlaylist"
      @cancel="pendingDeletePlaylistId = null"
    />
    <ConfirmModal
      :open="pendingRemoveTrackId !== null"
      title="Remove track?"
      body="The track will be removed from your collection along with its saved BPM and grid data."
      confirm-label="Remove"
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
        <button class="context-menu__item" @click="onContextMenuReanalyze">Recalculate BPM</button>
        <template v-if="store.playlists.length > 0">
          <div class="context-menu__item context-menu__item--sub" @mouseenter="onSubEnter">
            <span>Add to playlist</span>
            <span class="context-menu__arrow">▶</span>
            <div
              class="context-menu__submenu"
              :class="{ 'context-menu__submenu--flip': subFlipped }"
            >
              <button
                v-for="playlist in store.playlists"
                :key="playlist.id"
                class="context-menu__item"
                @click="onContextMenuAddToPlaylist(playlist.id)"
              >
                {{ playlist.name }}
              </button>
            </div>
          </div>
        </template>
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
import { ref, computed, nextTick } from 'vue';
import { useCollectionStore } from '@renderer/stores/collection';
import { useDecksStore, DECKS_DISPOSITION } from '@renderer/stores/decks';
import type { CollectionEntry } from '@renderer/stores/collection';
import type { DeckId } from '@renderer/stores/decks';
import BpmModal from '@renderer/components/BpmModal.vue';
import ConfirmModal from '@renderer/components/ConfirmModal.vue';

const store = useCollectionStore();
const decksStore = useDecksStore();

const isDragOver = ref(false);
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

function showDropBefore(idx: number): boolean {
  if (playlistDragFromIdx.value === null || playlistDropIdx.value !== idx) return false;
  if (playlistDropIdx.value === playlistDragFromIdx.value) return false;
  if (playlistDropIdx.value === playlistDragFromIdx.value + 1) return false;
  return true;
}

const showDropAfter = computed((): boolean => {
  if (playlistDragFromIdx.value === null) return false;
  if (playlistDropIdx.value !== playlistItems.value.length) return false;
  if (playlistDragFromIdx.value === playlistItems.value.length - 1) return false;
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

function deckHasTrack(deckId: string, path: string | null): boolean {
  if (!path) return false;
  return decksStore.decks[deckId as DeckId].loadedPath === path;
}

function loadToDeck(path: string, deckId: string) {
  window.dispatchEvent(new CustomEvent('bm:collection-drop', { detail: { deckId, path } }));
}

function removeFromActivePlaylist(path: string) {
  if (activePlaylistId.value) store.removeFromPlaylist(activePlaylistId.value, path);
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
  const target = decksStore.bestAvailableDeck();
  if (!target) return;
  loadToDeck(track.path, target);
}

function onTrackDblClickByPath(path: string) {
  const entry = store.tracks.find((t) => t.path === path);
  if (!entry || entry.status !== 'ready') return;
  const target = decksStore.bestAvailableDeck();
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
    const items = el.querySelectorAll('.collection__playlist-track');
    for (let i = 0; i < items.length; i++) {
      const rect = items[i].getBoundingClientRect();
      if (clientY < rect.top + rect.height / 2) return i;
    }
    return items.length;
  }

  function onMove(ev: PointerEvent) {
    playlistDropIdx.value = computeDropIdx(ev.clientY);
  }

  function onUp(ev: PointerEvent) {
    cleanup();
    const from = playlistDragFromIdx.value;
    const dropIdx = computeDropIdx(ev.clientY);
    playlistDragFromIdx.value = null;
    playlistDropIdx.value = null;
    if (from === null) return;
    if (dropIdx === from || dropIdx === from + 1) return;
    const to = dropIdx > from ? dropIdx - 1 : dropIdx;
    store.moveInPlaylist(playlist?.id ?? null, from, to);
  }

  function onCancel() {
    cleanup();
    playlistDragFromIdx.value = null;
    playlistDropIdx.value = null;
  }

  function cleanup() {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    window.removeEventListener('pointercancel', onCancel);
  }

  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onUp);
  window.addEventListener('pointercancel', onCancel);
}

function isAudio(file: File): boolean {
  return file.type.startsWith('audio/') || /\.(mp3|wav|flac|aac|ogg|m4a|aiff?)$/i.test(file.name);
}

async function readEntry(entry: FileSystemEntry): Promise<File[]> {
  if (entry.isFile) {
    return new Promise((resolve) => {
      (entry as FileSystemFileEntry).file(
        (f) => resolve(isAudio(f) ? [f] : []),
        () => resolve([])
      );
    });
  }
  if (entry.isDirectory) {
    const reader = (entry as FileSystemDirectoryEntry).createReader();
    const all: FileSystemEntry[] = [];
    while (true) {
      const batch = await new Promise<FileSystemEntry[]>((resolve) => {
        reader.readEntries(resolve, () => resolve([]));
      });
      if (batch.length === 0) break;
      all.push(...batch);
    }
    return (await Promise.all(all.map(readEntry))).flat();
  }
  return [];
}

function onDragOver(e: DragEvent) {
  if (store.draggingPath) return;
  e.preventDefault();
  isDragOver.value = true;
}

function onDragLeave() {
  isDragOver.value = false;
}

async function onDrop(e: DragEvent) {
  if (store.draggingPath) return;
  e.preventDefault();
  e.stopPropagation();
  isDragOver.value = false;
  const items = Array.from(e.dataTransfer?.items ?? []);
  const entries = items.map((i) => i.webkitGetAsEntry()).filter(Boolean) as FileSystemEntry[];
  const files = (await Promise.all(entries.map(readEntry))).flat();
  if (files.length > 0) store.addFiles(files);
}

// Movement below this threshold is treated as a click, not a drag start.
const DRAG_THRESHOLD = 5;

function onSearchPointerDown(e: PointerEvent) {
  if (store.draggingPath) e.preventDefault();
}

function onItemPointerDown(e: PointerEvent, track: CollectionEntry) {
  if (e.button !== 0 || track.status !== 'ready' || !track.path) return;
  if ((e.target as HTMLElement).closest('button')) return;

  const startX = e.clientX;
  const startY = e.clientY;
  const path = track.path;
  let active = false;

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
    }
  }

  function cleanup() {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    window.removeEventListener('pointercancel', onCancel);
  }

  function onUp(ev: PointerEvent) {
    cleanup();
    if (!active) return;
    document.body.style.cursor = '';
    const el = document.elementFromPoint(ev.clientX, ev.clientY);
    const deckEl = el?.closest('[data-deck-id]') as HTMLElement | null;
    const deckId = deckEl?.dataset.deckId;
    if (deckId) {
      window.dispatchEvent(new CustomEvent('bm:collection-drop', { detail: { deckId, path } }));
    }
    store.endDrag();
  }

  function onCancel() {
    cleanup();
    if (!active) return;
    document.body.style.cursor = '';
    store.endDrag();
  }

  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onUp);
  window.addEventListener('pointercancel', onCancel);
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
  border-left: none;
}

.collection__tab:hover {
  color: var(--color-text);
}

.collection__tab--active {
  border-color: #555;
  color: var(--color-text);
  background: var(--color-surface);
}

.collection__count {
  font-size: 0.8em;
  color: var(--color-muted);
  opacity: 0.6;
}

.collection__search-wrap {
  position: relative;
  display: flex;
  align-items: center;
}

.collection__search {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-text);
  font-family: var(--font);
  font-size: 0.8em;
  padding: 0.25em 1.6em 0.25em 0.5em;
  border-radius: 3px;
  outline: none;
  width: 8em;
}

.collection__search::placeholder {
  color: var(--color-muted);
  opacity: 0.5;
}

.collection__search:focus {
  border-color: #555;
}

.collection__search-clear {
  position: absolute;
  right: 0.3em;
  background: transparent;
  border: none;
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.72em;
  cursor: pointer;
  padding: 0;
  line-height: 1;
  opacity: 0.6;
}

.collection__search-clear:hover {
  opacity: 1;
  color: var(--color-text);
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
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
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
  color: #ef4444;
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

.collection__item-decks {
  display: flex;
  gap: 3px;
  flex-shrink: 0;
}

.collection__deck-btn {
  height: 1.6em;
  padding: 0 0.5em;
  border: 1px solid var(--btn-color);
  color: var(--btn-color);
  background: transparent;
  font-family: var(--font);
  font-size: 0.8em;
  font-weight: 700;
  border-radius: 2px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  white-space: nowrap;
  flex-shrink: 0;
  transition: background 0.1s;
}

.collection__deck-btn:hover:not(:disabled) {
  background: color-mix(in srgb, var(--btn-color) 20%, transparent);
}

.collection__deck-btn--loaded {
  background: color-mix(in srgb, var(--btn-color) 25%, transparent);
  cursor: default;
}

.collection__deck-btn:disabled {
  opacity: 0.35;
  cursor: default;
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
  opacity: 0.4;
  font-size: 0.9em;
  flex-shrink: 0;
  user-select: none;
}

.collection__drop-line {
  height: 2px;
  background: var(--color-text);
  opacity: 0.6;
  flex-shrink: 0;
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

.collection__add-search-wrap {
  position: relative;
  display: flex;
  align-items: center;
  padding: 6px 1em;
  border-bottom: 1px solid var(--color-border);
}

.collection__add-search-wrap .collection__search {
  width: 100%;
  padding-right: 1.6em;
}

.collection__add-search-wrap .collection__search-clear {
  right: calc(1em + 0.3em);
}
</style>

<style>
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
