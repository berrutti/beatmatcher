import { defineStore } from 'pinia';
import { reactive } from 'vue';

export type SavedTrack = {
  path: string;
  name: string;
  bpm: number;
  silenceEnd: number;
  beatOffset: number;
};

const STORAGE_KEY = 'beatmatcher:saved-tracks';

function loadFromStorage(): Record<string, SavedTrack> {
  try {
    const parsed = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}');
    return typeof parsed === 'object' && parsed !== null ? parsed : {};
  } catch {
    return {};
  }
}

export const useSavedTracksStore = defineStore('savedTracks', () => {
  const tracks = reactive<Record<string, SavedTrack>>(loadFromStorage());

  function persist() {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(tracks));
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

  return { tracks, get, save, update };
});
