<template>
  <div class="collection" :class="{ 'collection--drag-over': isDragOver }">
    <div class="collection__header">
      <div class="collection__tabs">
        <button
          tabindex="-1"
          class="collection__tab"
          :class="{ 'collection__tab--active': tab === 'all' }"
          @click="browse.setTab('all')"
        >
          {{ $t('browser.all') }}
        </button>
        <button
          tabindex="-1"
          class="collection__tab"
          :class="{ 'collection__tab--active': tab === 'playlists' }"
          @click="browse.setTab('playlists')"
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
          @click="browse.back()"
        >
          {{ $t('browser.back') }}
        </button>
      </template>
    </div>

    <AllTracksView v-if="tab === 'all'" :tracks="filteredTracks" />

    <div v-else-if="activePlaylistId === null" ref="overviewEl" class="collection__body">
      <div v-if="store.playlists.length === 0" class="collection__empty">
        {{ $t('browser.noPlaylists') }}
      </div>
      <!-- Fixed, non-resizable, non-reorderable columns: this list never
           grows past title + track count, so column customization would be
           pure overhead. -->
      <Table v-else>
        <template #colgroup>
          <col />
          <col :style="{ width: PLAYLIST_TRACK_COUNT_WIDTH + 'px' }" />
          <col :style="{ width: TABLE_CHROME_WIDTH.remove + 'px' }" />
        </template>
        <template #header>
          <TableHeaderCell>{{ $t('browser.colTitle') }}</TableHeaderCell>
          <TableHeaderCell>{{ $t('browser.colTracks') }}</TableHeaderCell>
          <TableHeaderCell></TableHeaderCell>
        </template>
        <tr
          v-for="playlist in store.playlists"
          :key="playlist.id"
          class="collection__row collection__item--playlist"
          :class="{ 'collection__item--cursor': isCursor(playlist.id) }"
          :data-row-key="playlist.id"
          @click="openPlaylist(playlist.id)"
        >
          <td class="collection__td collection__td--title">
            <span class="collection__item-name" v-tooltip="playlist.name">{{ playlist.name }}</span>
          </td>
          <td class="collection__td collection__td--bpm">
            {{ $t('browser.trackCount', playlist.paths.length) }}
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
          <td class="collection__td"></td>
        </tr>
      </Table>
    </div>

    <PlaylistDetailView v-else :playlist-id="activePlaylistId" />

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
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { useCollectionStore } from '@renderer/stores/collection';
import { useBrowseStore } from '@renderer/stores/browse';
import { useRowCursor } from '@renderer/composables/useRowCursor';
import { matchesTrackQuery } from '@renderer/utils/trackSearch';
import { displayName } from '@renderer/utils/trackDisplay';
import { TABLE_CHROME_WIDTH } from '@renderer/composables/useColumnResize';
import Search from '@renderer/components/collection/Search.vue';
import Table from '@renderer/components/collection/Table.vue';
import TableHeaderCell from '@renderer/components/collection/TableHeaderCell.vue';
import ConfirmModal from '@renderer/components/modals/ConfirmModal.vue';
import AllTracksView from '@renderer/components/collection/AllTracksView.vue';
import PlaylistDetailView from '@renderer/components/collection/PlaylistDetailView.vue';

const store = useCollectionStore();
const browse = useBrowseStore();

const isDragOver = ref(false);
const pendingClear = ref(false);
const searchQuery = ref('');

const tab = computed(() => browse.tab);
const activePlaylistId = computed(() => browse.activePlaylistId);
const overviewEl = ref<HTMLElement | null>(null);
// Named rather than a flag so navigating away closes the editor on its own,
// including when the controller is what navigated.
const renamingPlaylistId = ref<string | null>(null);
const renamingPlaylist = computed(
  () => renamingPlaylistId.value !== null && renamingPlaylistId.value === activePlaylistId.value
);
const renameValue = ref('');
const renameInputEl = ref<HTMLInputElement | null>(null);

const pendingDeletePlaylistId = ref<string | null>(null);

const pendingDeletePlaylistName = computed(
  () => store.playlists.find((p) => p.id === pendingDeletePlaylistId.value)?.name ?? ''
);

const filteredTracks = computed(() => {
  const q = searchQuery.value;
  if (!q.trim()) return store.tracks;
  return store.tracks.filter((t) => matchesTrackQuery(t, displayName(t.name), q));
});

// Unrelated to the pinned bpm width it happens to match: this table has no
// column customization at all.
const PLAYLIST_TRACK_COUNT_WIDTH = 55;

function confirmClear() {
  store.clearAll();
  pendingClear.value = false;
}

function confirmDeletePlaylist() {
  if (pendingDeletePlaylistId.value) {
    if (activePlaylistId.value === pendingDeletePlaylistId.value) browse.back();
    store.deletePlaylist(pendingDeletePlaylistId.value);
  }
  pendingDeletePlaylistId.value = null;
}

const activePlaylist = computed(
  () => store.playlists.find((p) => p.id === activePlaylistId.value) ?? null
);

function openPlaylist(id: string) {
  browse.openPlaylist(id);
}

const { cursorKey, isCursor } = useRowCursor(
  () => 'playlists',
  () => store.playlists.map((playlist) => playlist.id)
);

watch(cursorKey, async (key) => {
  if (key === null) return;
  await nextTick();
  const row = overviewEl.value?.querySelector(`[data-row-key="${CSS.escape(key)}"]`);
  row?.scrollIntoView({ block: 'nearest' });
});

async function onCreatePlaylist() {
  store.createPlaylist(`Playlist ${store.playlists.length + 1}`);
  const created = store.playlists[store.playlists.length - 1];
  browse.openPlaylist(created.id);
  renameValue.value = created.name;
  renamingPlaylistId.value = created.id;
  await nextTick();
  renameInputEl.value?.select();
}

async function startRename() {
  const p = activePlaylist.value;
  if (!p) return;
  renameValue.value = p.name;
  renamingPlaylistId.value = p.id;
  await nextTick();
  renameInputEl.value?.select();
}

function confirmRename() {
  const name = renameValue.value.trim();
  if (name && activePlaylistId.value) {
    store.renamePlaylist(activePlaylistId.value, name);
  }
  renamingPlaylistId.value = null;
}

function cancelRename() {
  renamingPlaylistId.value = null;
}

const AUDIO_EXT = /\.(mp3|wav|flac|aac|ogg|m4a|aiff?)$/i;

// Tauri's native drag-drop, because HTML5 DnD cannot see absolute paths in v2.
// An internal track drag is pointer-based and never reaches here.
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

<style>
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
  text-align: right;
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

.collection__item--played {
  color: var(--color-muted);
}

.collection__item--played .collection__item-name {
  color: var(--color-muted);
}

.collection__item--cursor {
  background: var(--color-surface);
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

.collection__sort-btn {
  width: 100%;
  background: transparent;
  border: none;
  color: inherit;
  font-family: var(--font);
  font-size: 1em;
  letter-spacing: inherit;
  text-transform: inherit;
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
  /* Without this, a mousedown-and-move on a row's text starts a native
     text-selection drag instead of (or alongside) our own pointer-based
     drag-to-deck/reorder logic - and once the pointer nears the top of the
     scrollable list, the browser auto-scrolls to extend that selection,
     which looks exactly like the list scrolling on its own mid-drag. */
  user-select: none;
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
  border-right: 1px solid var(--color-border);
  padding: 0 4px;
  overflow: hidden;
}

.collection__td--bpm,
.collection__td--added {
  text-align: left;
}

.collection__td--meta {
  white-space: nowrap;
  text-overflow: ellipsis;
}

.collection__meta-value {
  color: var(--color-muted);
}

/* The title is what a DJ reads down the list. The rest of the metadata is
   there to be scanned, so only this column carries full contrast. */
.collection__meta-value--title {
  color: var(--color-text);
}

.collection__item--played .collection__meta-value--title {
  color: var(--color-muted);
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
  transition: opacity 90ms ease;
}
.collection__table--reordering .collection__playlist-track {
  transition: transform 120ms ease;
}

.collection__table--reordering,
.collection__table--reordering * {
  cursor: grabbing !important;
}

.collection__playlist-track:active {
  cursor: grabbing;
}

.collection__playlist-track--dragging {
  opacity: 0.25;
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

.collection__playlist-handle {
  cursor: grab;
  text-align: center;
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

/* One flex row for every item, so a trailing element aligns itself rather than
   each variant restating the layout. */
.context-menu__item {
  display: flex;
  align-items: center;
  gap: 8px;
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

.context-menu__title {
  padding: 4px 14px 6px;
  font-size: 0.65rem;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--color-muted);
}

/* The width is always reserved, so ticking never resizes the menu. */
.context-menu__checkbox {
  margin-left: auto;
  width: 1em;
  text-align: center;
  color: var(--color-accent-cyan);
}

.context-menu__item--sub {
  position: relative;
  cursor: default;
}

.context-menu__item--disabled {
  cursor: not-allowed;
  opacity: 0.45;
}

.context-menu__item--disabled:hover {
  background: none;
  color: var(--color-text);
}

.context-menu__item-hint {
  margin-left: auto;
  font-size: 0.85em;
  white-space: nowrap;
}

.context-menu__arrow {
  margin-left: auto;
  font-size: 0.6em;
  opacity: 0.5;
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
