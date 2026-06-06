// All localStorage keys the app uses, in one place.
// Format: beatmatcher.<group>.<item>all lowercase kebab-case.
export const STORAGE_KEYS = {
  // User-facing settings persisted across sessions
  settings: 'beatmatcher.settings',

  // Collection and track metadata
  collection: 'beatmatcher.library.collection',
  playlists: 'beatmatcher.library.playlists',
  savedTracks: 'beatmatcher.library.saved-tracks',

  // UI state (layout, panel sizes)
  deckCount: 'beatmatcher.ui.deck-count',
  collectionHeight: 'beatmatcher.ui.collection-height'
} as const;

export function storageGet<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

export function storageSet<T>(key: string, value: T): void {
  localStorage.setItem(key, JSON.stringify(value));
}
