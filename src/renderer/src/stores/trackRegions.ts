const STORAGE_KEY = 'beatmatcher:track-regions';

export type SavedRegion = {
  startSec: number;
  endSec: number;
  beats: 16 | 32;
  detectedBpm: number;
};

function fileKey(file: File): string {
  return `${file.name}/${file.size}`;
}

function loadAll(): Record<string, SavedRegion> {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}');
  } catch {
    return {};
  }
}

export function saveRegion(file: File, region: SavedRegion) {
  const all = loadAll();
  all[fileKey(file)] = region;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
}

export function getSavedRegion(file: File): SavedRegion | null {
  const saved = loadAll()[fileKey(file)];
  if (!saved || !saved.detectedBpm) return null;
  return saved;
}
