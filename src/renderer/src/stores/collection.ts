import { defineStore } from 'pinia';
import { reactive, ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useSavedTracksStore } from '@renderer/stores/savedTracks';
import type { LoadableTrack } from '@renderer/stores/decks';

export type CollectionEntryStatus = 'idle' | 'analyzing' | 'ready' | 'error' | 'missing';

export type CollectionEntry = {
  id: string;
  name: string;
  size: number;
  path: string | null;
  status: CollectionEntryStatus;
  silenceEnd: number;
};

type PersistedEntry = { name: string; size: number; path: string | null };

const COLLECTION_KEY = 'beatmatcher:collection';

function loadPersisted(): PersistedEntry[] {
  try {
    return JSON.parse(localStorage.getItem(COLLECTION_KEY) ?? '[]');
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
      silenceEnd: 0
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
      tracks.push({
        id: `${name}-${Math.random().toString(36).slice(2)}`,
        name,
        size,
        path,
        status: hasSaved ? 'ready' : 'idle',
        silenceEnd: 0
      });
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
        }
        continue;
      }
      const hasSaved = path !== null && savedTracks.get(path) !== null;
      tracks.push({
        id: `${file.name}-${Math.random().toString(36).slice(2)}`,
        name: file.name,
        size: file.size,
        path,
        status: hasSaved ? 'ready' : 'idle',
        silenceEnd: 0
      });
    }
  }

  function removeTrack(id: string) {
    const idx = tracks.findIndex((t) => t.id === id);
    if (idx !== -1) tracks.splice(idx, 1);
  }

  function clearAll() {
    tracks.splice(0, tracks.length);
  }

  async function analyzeTrack(id: string) {
    const entry = tracks.find((t) => t.id === id);
    if (!entry || !entry.path || entry.status === 'analyzing' || entry.status === 'missing') return;
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

  function analyzeAll() {
    for (const t of tracks.filter((t) => t.status === 'idle')) {
      analyzeTrack(t.id);
    }
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

  function updateTrack(path: string, patch: { beatOffset?: number }) {
    savedTracks.update(path, patch);
  }

  function getLoadable(path: string): Omit<LoadableTrack, 'onBeatOffsetChange'> | null {
    const saved = savedTracks.get(path);
    if (!saved) return null;
    return {
      path,
      name: saved.name,
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

  return {
    isOpen,
    tracks,
    draggingPath,
    hasPending,
    bpmFor,
    toggle,
    addFiles,
    addFilesFromPaths,
    removeTrack,
    clearAll,
    analyzeTrack,
    analyzeAll,
    setBpm,
    updateTrack,
    getLoadable,
    startDrag,
    endDrag
  };
});
