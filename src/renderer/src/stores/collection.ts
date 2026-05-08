import { defineStore } from 'pinia';
import { reactive, ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useSavedTracksStore } from '@renderer/stores/savedTracks';
import type { LoadableTrack } from '@renderer/stores/decks';

export type CollectionEntryStatus = 'idle' | 'analyzing' | 'ready' | 'error' | 'missing';

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

const COLLECTION_KEY = 'beatmatcher:collection';
const PLAYLISTS_KEY = 'beatmatcher:playlists';

function loadPersisted(): PersistedEntry[] {
  try {
    return JSON.parse(localStorage.getItem(COLLECTION_KEY) ?? '[]');
  } catch {
    return [];
  }
}

function loadPersistedPlaylists(): Playlist[] {
  try {
    return JSON.parse(localStorage.getItem(PLAYLISTS_KEY) ?? '[]');
  } catch {
    return [];
  }
}

function persist(entries: CollectionEntry[]) {
  const data: PersistedEntry[] = entries.map((t) => ({
    name: t.name,
    size: t.size,
    path: t.path
  }));
  localStorage.setItem(COLLECTION_KEY, JSON.stringify(data));
}

export const useCollectionStore = defineStore('collection', () => {
  const savedTracks = useSavedTracksStore();
  const isOpen = ref(false);
  const tracks = reactive<CollectionEntry[]>([]);
  const draggingPath = ref<string | null>(null);

  const hasPending = computed(() => tracks.some((t) => t.status === 'idle'));

  function bpmFor(entry: CollectionEntry): number | null {
    return entry.path ? (savedTracks.get(entry.path)?.bpm ?? null) : null;
  }

  for (const p of loadPersisted()) {
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

  if (tracks.length > 0) {
    const pathsWithIdx = tracks
      .map((t, i) => (t.path ? { path: t.path, i } : null))
      .filter((x): x is { path: string; i: number } => x !== null);
    if (pathsWithIdx.length > 0) {
      invoke<(number | null)[]>('files_info', { paths: pathsWithIdx.map((x) => x.path) })
        .then((sizes) => {
          pathsWithIdx.forEach(({ path, i }, k) => {
            const size = sizes[k];
            if (size === null || size === undefined) return;
            const entry = tracks[i];
            const hasSaved = savedTracks.get(path) !== null;
            entry.size = size;
            entry.status = hasSaved ? 'ready' : 'idle';
            queueTagRead(entry.id);
          });
        })
        .catch(() => {});
    }
  }

  watch(
    () => tracks.map((t) => ({ name: t.name, size: t.size, path: t.path })),
    () => persist(tracks),
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
      const hasSaved = savedTracks.get(path) !== null;
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
          const hasSaved = existing.path !== null && savedTracks.get(existing.path) !== null;
          existing.status = hasSaved ? 'ready' : 'idle';
          queueTagRead(existing.id);
        }
        continue;
      }
      const hasSaved = path !== null && savedTracks.get(path) !== null;
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
      if (path) savedTracks.remove(path);
      tracks.splice(idx, 1);
    }
  }

  function reanalyzeTrack(id: string) {
    const entry = tracks.find((t) => t.id === id);
    if (!entry || !entry.path) return;
    savedTracks.remove(entry.path);
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
        savedTracks.save({
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
    savedTracks.save({
      path: entry.path,
      name: entry.name,
      bpm,
      silenceEnd: entry.silenceEnd,
      beatOffset: entry.silenceEnd
    });
    entry.status = 'ready';
  }

  function updateTrack(path: string, patch: { beatOffset?: number; bpm?: number }) {
    savedTracks.update(path, patch);
  }

  function getLoadable(path: string): Omit<LoadableTrack, 'onBeatOffsetChange'> | null {
    const saved = savedTracks.get(path);
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

  function startDrag(path: string) {
    draggingPath.value = path;
  }

  function endDrag() {
    draggingPath.value = null;
  }

  const playlists = reactive<Playlist[]>(loadPersistedPlaylists());

  watch(
    playlists,
    () => {
      localStorage.setItem(PLAYLISTS_KEY, JSON.stringify(playlists));
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
    isOpen,
    tracks,
    draggingPath,
    hasPending,
    playlists,
    bpmFor,
    toggle,
    addFiles,
    addFilesFromPaths,
    removeTrack,
    clearAll,
    analyzeTrack,
    reanalyzeTrack,
    analyzeAll,
    setBpm,
    updateTrack,
    getLoadable,
    startDrag,
    endDrag,
    createPlaylist,
    deletePlaylist,
    renamePlaylist,
    addToPlaylist,
    removeFromPlaylist,
    moveInPlaylist
  };
});
