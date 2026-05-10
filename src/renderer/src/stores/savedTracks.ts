import { defineStore } from 'pinia';
import { reactive } from 'vue';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';

export type SavedTrack = {
  path: string;
  name: string;
  bpm: number;
  silenceEnd: number;
  beatOffset: number;
};

export const useSavedTracksStore = defineStore('savedTracks', () => {
  const stored = storageGet<Record<string, SavedTrack>>(STORAGE_KEYS.savedTracks, {});
  const tracks = reactive<Record<string, SavedTrack>>(
    typeof stored === 'object' && stored !== null ? stored : {}
  );

  function persist() {
    storageSet(STORAGE_KEYS.savedTracks, tracks);
  }

  function get(path: string): SavedTrack | null {
    return tracks[path] ?? null;
  }

  function save(track: SavedTrack) {
    tracks[track.path] = track;
    persist();
  }

  function update(path: string, patch: Partial<Omit<SavedTrack, 'path'>>) {
    const existing = tracks[path];
    if (!existing) return;
    tracks[path] = { ...existing, ...patch };
    persist();
  }

  function remove(path: string) {
    delete tracks[path];
    persist();
  }

  return { tracks, get, save, update, remove };
});
