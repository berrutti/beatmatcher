import { defineStore } from 'pinia';
import { reactive, ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { LoadableTrack } from '@renderer/stores/decks';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import { indexByBasename } from '@renderer/utils/path';

type CollectionEntryStatus = 'idle' | 'analyzing' | 'ready' | 'error' | 'missing';

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
    } catch (err) {
      console.error('[collection] failed to check initial file sizes:', err);
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

  function relinkEntry(entry: CollectionEntry, newPath: string, size: number) {
    if (tracks.some((t) => t.id !== entry.id && t.path === newPath)) {
      const idx = tracks.findIndex((t) => t.id === entry.id);
      if (idx !== -1) tracks.splice(idx, 1);
      return;
    }

    const oldPath = entry.path;
    if (oldPath && oldPath !== newPath) {
      const saved = savedTracks[oldPath];
      if (saved) {
        savedTracks[newPath] = { ...saved, path: newPath };
        delete savedTracks[oldPath];
        persistSaved();
      }
      for (const p of playlists) {
        const idx = p.paths.indexOf(oldPath);
        if (idx !== -1) p.paths[idx] = newPath;
      }
    }

    entry.path = newPath;
    entry.size = size;
    entry.status = getSaved(newPath) !== null ? 'ready' : 'idle';
    queueTagRead(entry.id);
  }

  // Opens a folder picker and relinks every missing entry whose filename is
  // found under it (recursively), so one pick fixes a whole moved library.
  // Saved BPM/grid data and playlist references follow the path, so nothing
  // has to be re-analyzed.
  async function locateMissingTracks(): Promise<void> {
    if (!tracks.some((t) => t.status === 'missing')) return;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const folder = await open({ directory: true, multiple: false });
    if (typeof folder !== 'string') return;

    const found = await invoke<string[]>('scan_folder', { path: folder }).catch(
      () => [] as string[]
    );
    const byName = indexByBasename(found);

    const targets = tracks
      .filter((t) => t.status === 'missing')
      .map((t) => ({ entry: t, newPath: byName.get(t.name) }))
      .filter((t): t is { entry: CollectionEntry; newPath: string } => t.newPath !== undefined);
    if (targets.length === 0) return;

    const sizes = await invoke<(number | null)[]>('files_info', {
      paths: targets.map((t) => t.newPath)
    });
    targets.forEach(({ entry: target, newPath }, i) => {
      const size = sizes[i];
      if (size !== null && size !== undefined) relinkEntry(target, newPath, size);
    });
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

  function createConcurrentQueue(concurrency: number, worker: (id: string) => Promise<void>) {
    let active = 0;
    const pending: string[] = [];

    function drain() {
      while (active < concurrency && pending.length > 0) {
        const id = pending.shift();
        if (id === undefined) continue;
        active++;
        run(id);
      }
    }

    async function run(id: string) {
      await worker(id);
      active--;
      drain();
    }

    return {
      enqueue(id: string) {
        pending.push(id);
        drain();
      },
      get pending() {
        return pending;
      }
    };
  }

  async function doAnalyze(id: string) {
    const entry = tracks.find((t) => t.id === id);
    if (!entry || !entry.path || entry.status !== 'idle') return;
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

  const analysisQueue = createConcurrentQueue(3, doAnalyze);
  const tagQueue = createConcurrentQueue(8, async (id) => {
    const entry = tracks.find((t) => t.id === id);
    if (entry) await readTagsForEntry(entry);
  });

  function analyzeTrack(id: string) {
    const entry = tracks.find((t) => t.id === id);
    if (!entry || !entry.path || entry.status === 'analyzing' || entry.status === 'missing') return;
    analysisQueue.enqueue(id);
  }

  function analyzeAll() {
    for (const track of tracks.filter((t) => t.status === 'idle')) {
      analysisQueue.enqueue(track.id);
    }
  }

  function queueTagRead(id: string) {
    tagQueue.enqueue(id);
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

  function getName(path: string): string {
    const entry = tracks.find((t) => t.path === path);
    if (entry) return entry.title ?? entry.name;
    return (
      path
        .split('/')
        .pop()
        ?.replace(/\.[^.]+$/, '') ?? path
    );
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
    getName,
    getSaved,
    getLoadableTrack,
    locateMissingTracks,
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
