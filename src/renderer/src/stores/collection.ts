import { defineStore } from 'pinia';
import { reactive, ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { LoadableTrack } from '@renderer/stores/decks';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';

export type CollectionEntryStatus = 'idle' | 'analyzing' | 'ready' | 'error' | 'missing';

export type SavedTrack = {
  path: string;
  name: string;
  bpm: number;
  silenceEnd: number;
  beatOffset: number;
};

export type Playlist = {
  id: string;
  name: string;
  paths: string[];
};

export type CollectionEntry = {
  id: string;
  name: string;
  size: number;
  path: string | null;
  status: CollectionEntryStatus;
  silenceEnd: number;
  title: string | null;
};

type PersistedEntry = { name: string; size: number; path: string | null };

function persistCollection(entries: CollectionEntry[]) {
  storageSet(
    STORAGE_KEYS.collection,
    entries.map((t) => ({ name: t.name, size: t.size, path: t.path }))
  );
}

export const useCollectionStore = defineStore('collection', () => {
  const stored = storageGet<Record<string, SavedTrack>>(STORAGE_KEYS.savedTracks, {});
  const savedTracks = reactive<Record<string, SavedTrack>>(
    typeof stored === 'object' && stored !== null ? stored : {}
  );

  function persistSaved() {
    storageSet(STORAGE_KEYS.savedTracks, savedTracks);
  }

  function getSaved(path: string): SavedTrack | null {
    return savedTracks[path] ?? null;
  }

  function saveSaved(track: SavedTrack) {
    savedTracks[track.path] = track;
    persistSaved();
  }

  function updateSaved(path: string, patch: Partial<Omit<SavedTrack, 'path'>>) {
    const existing = savedTracks[path];
    if (!existing) return;
    savedTracks[path] = { ...existing, ...patch };
    persistSaved();
  }

  function removeSaved(path: string) {
    delete savedTracks[path];
    persistSaved();
  }

  const isOpen = ref(false);
  const tracks = reactive<CollectionEntry[]>([]);
  const draggingPath = ref<string | null>(null);

  const hasPending = computed(() => tracks.some((t) => t.status === 'idle'));

  function getBpm(entry: CollectionEntry): number | null {
    return entry.path ? (getSaved(entry.path)?.bpm ?? null) : null;
  }

  for (const p of storageGet<PersistedEntry[]>(STORAGE_KEYS.collection, [])) {
    tracks.push({
      id: `${p.name}-${Math.random().toString(36).slice(2)}`,
      name: p.name,
      size: p.size,
      path: p.path,
      status: 'missing',
      silenceEnd: 0,
      title: null
    });
  }

  async function checkInitialFileSizes() {
    const pathsWithIdx = tracks
      .map((t, i) => (t.path ? { path: t.path, i } : null))
      .filter((x): x is { path: string; i: number } => x !== null);
    if (pathsWithIdx.length === 0) return;
    try {
      const sizes = await invoke<(number | null)[]>('files_info', {
        paths: pathsWithIdx.map((x) => x.path)
      });
      pathsWithIdx.forEach(({ path, i }, k) => {
        const size = sizes[k];
        if (size === null || size === undefined) return;
        const entry = tracks[i];
        const hasSaved = getSaved(path) !== null;
        entry.size = size;
        entry.status = hasSaved ? 'ready' : 'idle';
        queueTagRead(entry.id);
      });
    } catch {
      // ignore, tracks stay in 'missing' state
    }
  }

  if (tracks.length > 0) checkInitialFileSizes();

  watch(
    () => tracks.map((t) => ({ name: t.name, size: t.size, path: t.path })),
    () => persistCollection(tracks),
    { deep: true }
  );

  function toggle() {
    isOpen.value = !isOpen.value;
  }

  async function addFilesFromPaths(paths: string[]) {
    const newPaths = paths.filter((p) => !tracks.some((t) => t.path === p));
    if (newPaths.length === 0) return;
    const sizes = await invoke<(number | null)[]>('files_info', { paths: newPaths });
    newPaths.forEach((path, i) => {
      const size = sizes[i];
      if (size === null || size === undefined) return;
      const name = path.split('/').pop() ?? path;
      const hasSaved = getSaved(path) !== null;
      const entry: CollectionEntry = {
        id: `${name}-${Math.random().toString(36).slice(2)}`,
        name,
        size,
        path,
        status: hasSaved ? 'ready' : 'idle',
        silenceEnd: 0,
        title: null
      };
      tracks.push(entry);
      queueTagRead(entry.id);
    });
  }

  function addFiles(files: File[]) {
    for (const file of files) {
      const path = (file as File & { path?: string }).path ?? null;
      const existing = tracks.find((t) => t.name === file.name && t.size === file.size);
      if (existing) {
        if (existing.status === 'missing') {
          existing.path = path ?? existing.path;
          const hasSaved = existing.path !== null && getSaved(existing.path) !== null;
          existing.status = hasSaved ? 'ready' : 'idle';
          queueTagRead(existing.id);
        }
        continue;
      }
      const hasSaved = path !== null && getSaved(path) !== null;
      const entry: CollectionEntry = {
        id: `${file.name}-${Math.random().toString(36).slice(2)}`,
        name: file.name,
        size: file.size,
        path,
        status: hasSaved ? 'ready' : 'idle',
        silenceEnd: 0,
        title: null
      };
      tracks.push(entry);
      queueTagRead(entry.id);
    }
  }

  function removeTrack(id: string) {
    const idx = tracks.findIndex((t) => t.id === id);
    if (idx !== -1) {
      const path = tracks[idx].path;
      if (path) removeSaved(path);
      tracks.splice(idx, 1);
    }
  }

  function reanalyzeTrack(id: string) {
    const entry = tracks.find((t) => t.id === id);
    if (!entry || !entry.path) return;
    removeSaved(entry.path);
    entry.status = 'idle';
    analyzeTrack(id);
  }

  function clearAll() {
    tracks.splice(0, tracks.length);
  }

  const ANALYZE_CONCURRENCY = 3;
  let activeAnalyses = 0;
  const analysisQueue: string[] = [];

  function drainQueue() {
    while (activeAnalyses < ANALYZE_CONCURRENCY && analysisQueue.length > 0) {
      const id = analysisQueue.shift()!;
      const entry = tracks.find((t) => t.id === id);
      if (!entry || entry.status !== 'idle') continue;
      activeAnalyses++;
      doAnalyze(id).finally(() => {
        activeAnalyses--;
        drainQueue();
      });
    }
  }

  async function doAnalyze(id: string) {
    const entry = tracks.find((t) => t.id === id);
    if (!entry || !entry.path) return;
    entry.status = 'analyzing';
    try {
      const result = await invoke<{ bpm: number | null; silenceEnd: number }>('analyze_track', {
        path: entry.path
      });
      entry.silenceEnd = result.silenceEnd;
      if (result.bpm !== null && result.bpm > 0) {
        saveSaved({
          path: entry.path,
          name: entry.name,
          bpm: result.bpm,
          silenceEnd: result.silenceEnd,
          beatOffset: result.silenceEnd
        });
        entry.status = 'ready';
      } else {
        entry.status = 'error';
      }
    } catch {
      entry.status = 'error';
    }
  }

  function analyzeTrack(id: string) {
    const entry = tracks.find((t) => t.id === id);
    if (!entry || !entry.path || entry.status === 'analyzing' || entry.status === 'missing') return;
    analysisQueue.push(id);
    drainQueue();
  }

  function analyzeAll() {
    for (const t of tracks.filter((t) => t.status === 'idle')) {
      analysisQueue.push(t.id);
    }
    drainQueue();
  }

  const TAG_READ_CONCURRENCY = 8;
  let activeTagReads = 0;
  const tagReadQueue: string[] = [];

  function drainTagQueue() {
    while (activeTagReads < TAG_READ_CONCURRENCY && tagReadQueue.length > 0) {
      const id = tagReadQueue.shift()!;
      const entry = tracks.find((t) => t.id === id);
      if (!entry || !entry.path) continue;
      activeTagReads++;
      readTagsForEntry(entry).finally(() => {
        activeTagReads--;
        drainTagQueue();
      });
    }
  }

  async function readTagsForEntry(entry: CollectionEntry) {
    if (!entry.path) return;
    try {
      const tags = await invoke<{ title: string | null; artist: string | null }>(
        'read_track_tags',
        { path: entry.path }
      );
      const parts = [tags.artist, tags.title].filter(Boolean);
      if (parts.length > 0) entry.title = parts.join(' - ');
    } catch {
      // ignore tag read failures
    }
  }

  function queueTagRead(id: string) {
    tagReadQueue.push(id);
    drainTagQueue();
  }

  function setBpm(id: string, bpm: number) {
    const entry = tracks.find((t) => t.id === id);
    if (!entry || !entry.path || bpm <= 0) return;
    saveSaved({
      path: entry.path,
      name: entry.name,
      bpm,
      silenceEnd: entry.silenceEnd,
      beatOffset: entry.silenceEnd
    });
    entry.status = 'ready';
  }

  function updateTrack(path: string, patch: { beatOffset?: number; bpm?: number }) {
    updateSaved(path, patch);
  }

  function getLoadable(path: string): Omit<LoadableTrack, 'onBeatOffsetChange'> | null {
    const saved = getSaved(path);
    if (!saved) return null;
    const entry = tracks.find((t) => t.path === path);
    return {
      path,
      name: entry?.title ?? saved.name,
      bpm: saved.bpm,
      silenceEnd: saved.silenceEnd,
      beatOffset: saved.beatOffset
    };
  }

  function getLoadableTrack(path: string): LoadableTrack | null {
    const data = getLoadable(path);
    return data
      ? {
          ...data,
          onBeatOffsetChange: (sec) => updateTrack(path, { beatOffset: sec })
        }
      : null;
  }

  async function scanFolders(folders: string[]): Promise<string[]> {
    const pathLists = await Promise.all(
      folders.map((folder) => invoke<string[]>('scan_folder', { path: folder }))
    );
    return pathLists.flat();
  }

  function startDrag(path: string) {
    draggingPath.value = path;
  }

  function endDrag() {
    draggingPath.value = null;
  }

  const playlists = reactive<Playlist[]>(storageGet<Playlist[]>(STORAGE_KEYS.playlists, []));

  watch(
    playlists,
    () => {
      storageSet(STORAGE_KEYS.playlists, playlists);
    },
    { deep: true }
  );

  function createPlaylist(name: string) {
    playlists.push({ id: Math.random().toString(36).slice(2), name, paths: [] });
  }

  function deletePlaylist(id: string) {
    const idx = playlists.findIndex((p) => p.id === id);
    if (idx !== -1) playlists.splice(idx, 1);
  }

  function renamePlaylist(id: string, name: string) {
    const p = playlists.find((p) => p.id === id);
    if (p) p.name = name;
  }

  function addToPlaylist(playlistId: string, path: string) {
    const p = playlists.find((p) => p.id === playlistId);
    if (!p || p.paths.includes(path)) return;
    p.paths.push(path);
  }

  function removeFromPlaylist(playlistId: string, path: string) {
    const p = playlists.find((p) => p.id === playlistId);
    if (!p) return;
    const idx = p.paths.indexOf(path);
    if (idx !== -1) p.paths.splice(idx, 1);
  }

  function moveInPlaylist(playlistId: string | null, fromIdx: number, toIdx: number) {
    const p = playlists.find((p) => p.id === playlistId);
    if (!p || fromIdx === toIdx) return;
    const [item] = p.paths.splice(fromIdx, 1);
    p.paths.splice(toIdx, 0, item);
  }

  return {
    draggingPath,
    hasPending,
    isOpen,
    playlists,
    tracks,
    addFiles,
    addFilesFromPaths,
    addToPlaylist,
    analyzeAll,
    analyzeTrack,
    clearAll,
    createPlaylist,
    deletePlaylist,
    endDrag,
    getBpm,
    getLoadableTrack,
    moveInPlaylist,
    reanalyzeTrack,
    removeFromPlaylist,
    removeTrack,
    renamePlaylist,
    scanFolders,
    setBpm,
    startDrag,
    toggle,
    updateTrack
  };
});
