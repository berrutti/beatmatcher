import { defineStore } from 'pinia';
import { reactive, ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { saveRegion, getSavedRegion } from '@renderer/stores/trackRegions';

export type CollectionEntryStatus = 'idle' | 'analyzing' | 'ready' | 'error' | 'missing';

export type CollectionEntry = {
  id: string;
  file: File | null;
  name: string;
  size: number;
  path: string | null;
  status: CollectionEntryStatus;
  bpm: number | null;
};

type PersistedEntry = { name: string; size: number; path: string | null; bpm: number | null };

const COLLECTION_KEY = 'beatmatcher:collection';
const isTauri = '__TAURI_INTERNALS__' in window;

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
    path: t.path,
    bpm: t.bpm,
  }));
  localStorage.setItem(COLLECTION_KEY, JSON.stringify(data));
}

async function reloadFile(entry: CollectionEntry) {
  if (!entry.path) return;
  try {
    const bytes = await invoke<number[]>('read_file', { path: entry.path });
    const file = new File([new Uint8Array(bytes)], entry.name);
    entry.file = file;
    entry.status = entry.bpm !== null ? 'ready' : 'idle';
  } catch {
    // stays missing
  }
}

export const useCollectionStore = defineStore('collection', () => {
  const isOpen = ref(false);
  const tracks = reactive<CollectionEntry[]>([]);
  const draggingFile = ref<File | null>(null);
  const draggingPath = ref<string | null>(null);

  const hasPending = computed(() => tracks.some((t) => t.status === 'idle'));

  for (const p of loadPersisted()) {
    const entry: CollectionEntry = {
      id: `${p.name}-${Math.random().toString(36).slice(2)}`,
      file: null,
      name: p.name,
      size: p.size,
      path: p.path,
      status: 'missing',
      bpm: p.bpm,
    };
    tracks.push(entry);
    if (isTauri) reloadFile(entry);
  }

  watch(
    () => tracks.map((t) => ({ name: t.name, size: t.size, path: t.path, bpm: t.bpm })),
    () => persist(tracks),
    { deep: true },
  );

  function toggle() {
    isOpen.value = !isOpen.value;
  }

  async function addFilesFromPaths(paths: string[]) {
    await Promise.all(
      paths.map(async (path) => {
        if (tracks.some((t) => t.path === path)) return;
        const name = path.split('/').pop() ?? path;
        let bytes: number[];
        try {
          bytes = await invoke<number[]>('read_file', { path });
        } catch {
          return;
        }
        const file = new File([new Uint8Array(bytes)], name);
        const saved = getSavedRegion(file);
        tracks.push({
          id: `${name}-${Math.random().toString(36).slice(2)}`,
          file,
          name,
          size: file.size,
          path,
          status: saved !== null ? 'ready' : 'idle',
          bpm: saved?.detectedBpm ?? null,
        });
      }),
    );
  }

  function addFiles(files: File[]) {
    for (const file of files) {
      const path = (file as File & { path?: string }).path ?? null;
      const existing = tracks.find((t) => t.name === file.name && t.size === file.size);
      if (existing) {
        if (existing.status === 'missing') {
          existing.file = file;
          existing.path = path ?? existing.path;
          existing.status = existing.bpm !== null ? 'ready' : 'idle';
        }
        continue;
      }
      const saved = getSavedRegion(file);
      tracks.push({
        id: `${file.name}-${Math.random().toString(36).slice(2)}`,
        file,
        name: file.name,
        size: file.size,
        path,
        status: saved !== null ? 'ready' : 'idle',
        bpm: saved?.detectedBpm ?? null,
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
    if (!entry || !entry.file || !entry.path || entry.status === 'analyzing') return;
    entry.status = 'analyzing';
    try {
      const result = await invoke<{ bpm: number | null; silenceEnd: number }>('analyze_track', {
        path: entry.path,
      });
      if (result.bpm !== null && result.bpm > 0) {
        const dur = (16 / result.bpm) * 60;
        saveRegion(entry.file, {
          startSec: result.silenceEnd,
          endSec: result.silenceEnd + dur,
          beats: 16,
          detectedBpm: result.bpm,
        });
        entry.bpm = result.bpm;
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

  function startDrag(file: File, path: string | null = null) {
    draggingFile.value = file;
    draggingPath.value = path;
  }

  function endDrag() {
    draggingFile.value = null;
    draggingPath.value = null;
  }

  return {
    isOpen,
    tracks,
    draggingFile,
    draggingPath,
    hasPending,
    toggle,
    addFiles,
    addFilesFromPaths,
    removeTrack,
    clearAll,
    analyzeTrack,
    analyzeAll,
    startDrag,
    endDrag,
  };
});
