// All localStorage keys the app uses, in one place.
// Format: beatmatcher.<group>.<item>all lowercase kebab-case.
export const STORAGE_KEYS = {
  // User-facing settings persisted across sessions
  settings: 'beatmatcher.settings',

  // Collection and track metadata
  collection: 'beatmatcher.library.collection',
  playlists: 'beatmatcher.library.playlists',
  savedTracks: 'beatmatcher.library.saved-tracks',
  metadataOverrides: 'beatmatcher.library.metadata-overrides',

  // UI state (layout, panel sizes)
  deckCount: 'beatmatcher.ui.deck-count',
  bigLibrary: 'beatmatcher.ui.big-library',
  showWaveformStrip: 'beatmatcher.ui.show-waveform-strip',
  locale: 'beatmatcher.ui.locale',
  sessionDeckLane: 'beatmatcher.ui.session-deck-lane',
  sessionLaneHeight: 'beatmatcher.ui.session-lane-height',
  sessionMasterLane: 'beatmatcher.ui.session-master-lane',
  sessionWaveformHeight: 'beatmatcher.ui.session-waveform-height',
  browserColumns: 'beatmatcher.ui.browser-columns',
  skipDiscardConfirm: 'beatmatcher.ui.skip-recovery-discard-confirm',

  // Hardware
  midiDeckAssignments: 'beatmatcher.midi.deck-assignments'
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
