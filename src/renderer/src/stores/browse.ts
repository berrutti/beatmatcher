import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { loadToDeck } from '@renderer/utils/deckDrop';
import { confirmModal, cancelModal } from '@renderer/utils/activeModal';

export type BrowseTab = 'all' | 'playlists';

export function playlistListId(playlistId: string): string {
  return `playlist:${playlistId}`;
}

export const useBrowseStore = defineStore('browse', () => {
  const tab = ref<BrowseTab>('all');
  const activePlaylistId = ref<string | null>(null);

  const rows = ref<string[]>([]);
  // One anchor per list, so leaving a playlist and coming back finds the cursor
  // where it was left. A stale entry costs nothing: the lookup simply misses.
  const anchors = ref<Record<string, string>>({});

  const listId = computed(() => {
    if (tab.value === 'all') return 'all';
    if (activePlaylistId.value === null) return 'playlists';
    return playlistListId(activePlaylistId.value);
  });

  const onPlaylistList = computed(() => listId.value === 'playlists');

  // The anchor is the row's own key, so a re-sort moves the highlight with the
  // track rather than leaving it on whatever now occupies that position, and a
  // filter that excludes the anchored row reads as -1 without discarding it.
  const cursorIndex = computed(() => {
    const anchor = anchors.value[listId.value];
    return anchor === undefined ? -1 : rows.value.indexOf(anchor);
  });

  const cursorKey = computed(() => rows.value[cursorIndex.value] ?? null);

  // Keyed by list because the playlists overview lives in the always-mounted
  // Browser while the two track lists are children of it, so which registration
  // wins cannot be left to mount order.
  function setRows(from: string, keys: string[]): void {
    if (from !== listId.value) return;
    rows.value = keys;
  }

  function setTab(next: BrowseTab): void {
    tab.value = next;
  }

  // The playlist stays open across a toggle, so coming back lands where it was
  // left rather than at the overview.
  function toggleView(): void {
    tab.value = tab.value === 'all' ? 'playlists' : 'all';
  }

  function openPlaylist(id: string): void {
    tab.value = 'playlists';
    activePlaylistId.value = id;
  }

  function moveCursor(steps: number): void {
    if (rows.value.length === 0) return;
    const from = cursorIndex.value;
    const to =
      from === -1
        ? steps > 0
          ? 0
          : rows.value.length - 1
        : Math.min(rows.value.length - 1, Math.max(0, from + steps));
    anchors.value[listId.value] = rows.value[to];
  }

  // An open modal takes these two: a controller LOAD onto a playing deck opens
  // one, and nothing else on the surface could dismiss it.
  function enter(): void {
    if (confirmModal()) return;
    const key = cursorKey.value;
    if (!onPlaylistList.value || key === null) return;
    openPlaylist(key);
  }

  function back(): void {
    if (cancelModal()) return;
    if (activePlaylistId.value !== null) {
      activePlaylistId.value = null;
      return;
    }
    if (tab.value === 'playlists') tab.value = 'all';
  }

  function loadCursorInto(deckId: string): void {
    const path = cursorKey.value;
    if (onPlaylistList.value || path === null) return;
    loadToDeck(path, deckId);
  }

  let bridging: Promise<UnlistenFn[]> | null = null;

  // Rust forwards these rather than acting on them: which row is lit, and so
  // which track a load button loads, is only knowable here.
  async function startBridge(): Promise<void> {
    if (!bridging) {
      bridging = Promise.all([
        listen<number>('midi-browse', (event) => moveCursor(event.payload)),
        listen('midi-enter', () => enter()),
        listen('midi-back', () => back()),
        listen('midi-toggle-view', () => toggleView()),
        listen<string>('midi-load', (event) => loadCursorInto(event.payload))
      ]);
    }
    await bridging;
  }

  startBridge().catch((cause) => console.error(cause));

  return {
    tab,
    activePlaylistId,
    rows,
    listId,
    cursorIndex,
    cursorKey,
    setRows,
    setTab,
    toggleView,
    openPlaylist,
    moveCursor,
    enter,
    back,
    loadCursorInto,
    startBridge
  };
});
