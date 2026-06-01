import { defineStore } from 'pinia';
import { reactive, ref, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useSettingsStore } from '@renderer/stores/settings';

export type DeckId = 'A' | 'B' | 'C' | 'D' | 'E'; // Deck E is a special deck for Edit view

export const DECKS_DISPOSITION = ['C', 'A', 'B', 'D'] as const;

export type LoopRegion = {
  startSec: number;
  endSec: number;
  beats: number;
};

export type TrackData = {
  duration: number;
  sampleRate: number;
  bpm: number | null;
  silenceEnd: number;
  coverArt: string | null;
};

export type LoadableTrack = {
  path: string;
  name: string;
  bpm: number;
  silenceEnd: number;
  beatOffset: number;
  onBeatOffsetChange: (sec: number) => void;
};

export const EQ_MIN_DB = -26;
export const EQ_MAX_DB = 6;

// Dense LOD points-per-second. Sized to comfortably satisfy zoom >= ~5s on
// typical canvases (1000-2000px). For a 3-minute track this is ~650 KB
// of Float32 data; for 10 minutes ~2.1 MB. Anything zoomed deeper than the
// rate can cover (sub-second zoom levels) falls back to an on-demand fetch.
const DENSE_LOD_PTS_PER_SEC = 250;

type DeckSyncPayload = {
  isPlaying: boolean;
  isCueing: boolean;
  cuePointSec: number;
  positionSec: number;
  loopActive: boolean;
  loopRegionCleared: boolean;
};

function createDeck(id: DeckId, accent: string, name: string) {
  let positionCache = 0;
  let clockAtPlay = 0; // performance.now() when playback started or position was last anchored
  let localRate = 1.0; // effective playback rate (pitch + nudge) for interpolation
  let onBeatOffsetChangeCb: ((sec: number) => void) | null = null;
  let bandsReadyUnlisten: (() => void) | null = null;
  let loadGeneration = 0;

  async function fetchDenseLodChunked(
    generation: number,
    duration: number,
    totalPoints: number
  ): Promise<void> {
    const CHUNKS = 10;
    const buffer = new Float32Array(totalPoints * 4);
    for (let i = 0; i < CHUNKS; i++) {
      if (loadGeneration !== generation) return;
      const startPt = Math.floor((i * totalPoints) / CHUNKS);
      const endPt = Math.floor(((i + 1) * totalPoints) / CHUNKS);
      const chunkBuf = await state.getSpectralWaveformRegion(
        (duration * startPt) / totalPoints,
        (duration * endPt) / totalPoints,
        endPt - startPt
      );
      if (loadGeneration !== generation) return;
      buffer.set(new Float32Array(chunkBuf), startPt * 4);
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
    }
    if (loadGeneration !== generation) return;
    state.denseSpectralData = buffer;
    state.denseSpectralRate = totalPoints / duration;
  }

  // Re-anchor positionCache to now so rate changes don't cause a position jump.
  function syncPosition() {
    if (state.loopPlaying) {
      positionCache += ((performance.now() - clockAtPlay) / 1000) * localRate;
      clockAtPlay = performance.now();
      if (state.loopActive && state.loopRegion) {
        const { startSec, endSec } = state.loopRegion;
        const loopDur = endSec - startSec;
        if (loopDur > 0 && positionCache >= endSec) {
          positionCache = startSec + ((positionCache - startSec) % loopDur);
        }
      }
    }
  }

  function applyDeckState(payload: DeckSyncPayload) {
    state.loopPlaying = payload.isPlaying;
    state.cueing = payload.isCueing;
    state.cuePoint = payload.cuePointSec;
    state.loopActive = payload.loopActive;
    positionCache = payload.positionSec;
    if (payload.isPlaying) clockAtPlay = performance.now();
    if (payload.loopRegionCleared) {
      state.loopRegion = null;
    }
  }

  function interpolatedPosition(): number {
    let pos = positionCache;
    if (state.loopPlaying) {
      pos += ((performance.now() - clockAtPlay) / 1000) * localRate;
    }
    if (state.loopActive && state.loopRegion) {
      const { startSec, endSec } = state.loopRegion;
      const loopDur = endSec - startSec;
      if (loopDur > 0 && pos >= endSec) {
        pos = startSec + ((pos - startSec) % loopDur);
      }
    }
    if (state.trackData && pos > state.trackData.duration) {
      pos = state.trackData.duration;
    }
    return pos;
  }

  const state = reactive({
    id,
    accent,
    name,
    trackName: '',
    trackLoaded: false,
    loading: false,
    waveformLoading: false,
    loadedPath: null as string | null,
    trackData: null as TrackData | null,
    // Low-rate overview covering the whole track (few points per second).
    // Used by the overview strip and by WaveformDisplay as a first-paint
    // fallback while the dense LOD is still loading.
    fullSpectralData: null as Float32Array | null,
    // Higher-rate LOD covering the whole track. WaveformDisplay slices this
    // directly in JS for any zoom level the rate can satisfy, avoiding IPC
    // round-trips on pan/zoom. Deeper zoom levels fall back to on-demand
    // fetches; see WaveformDisplay for the switching logic.
    denseSpectralData: null as Float32Array | null,
    denseSpectralRate: 0,
    coverArt: null as string | null,
    loopPlaying: false,
    loopRegion: null as LoopRegion | null,
    loopActive: false,
    quantized: true,

    trackBpm: null as number | null,
    beatOffset: 0,
    cuePoint: 0,
    targetBpm: null as number | null,
    pitchOffset: 0,

    nudging: null as 'back' | 'forward' | null,
    cueing: false,
    eq: { low: 0, mid: 0, high: 0 },

    get trackPosition(): number | null {
      return state.loopPlaying ? interpolatedPosition() : null;
    },

    get phase(): number {
      if (state.trackBpm === null || !state.loopPlaying) return 0;
      const pos = interpolatedPosition();
      const beats = ((pos - state.beatOffset) * state.trackBpm) / 60;
      return ((beats % 1) + 1) % 1;
    },

    setTargetBpm(value: number) {
      if (state.trackBpm === null) return;
      const pitchRange = useSettingsStore().pitchRange;
      const minBpm = state.trackBpm * (1 - pitchRange / 100);
      const maxBpm = state.trackBpm * (1 + pitchRange / 100);
      const clamped = Math.max(minBpm, Math.min(maxBpm, value));
      state.targetBpm = clamped;
      state.pitchOffset = (clamped / state.trackBpm - 1) * 100;
      syncPosition();
      localRate = clamped / state.trackBpm;
      invoke('set_playback_rate', { deck: id, rate: localRate });
    },

    setTrackBpm(bpm: number) {
      state.trackBpm = bpm;
      state.targetBpm = bpm;
      state.pitchOffset = 0;
      syncPosition();
      localRate = 1.0;
      invoke('set_playback_rate', { deck: id, rate: 1.0 });
      invoke('set_beat_grid', { deck: id, bpm, beatOffsetSec: state.beatOffset });
    },

    setPitchOffset(pct: number) {
      if (state.trackBpm === null) return;
      const pitchRange = useSettingsStore().pitchRange;
      state.pitchOffset = Math.max(-pitchRange, Math.min(pitchRange, pct));
      state.targetBpm = state.trackBpm * (1 + state.pitchOffset / 100);
      syncPosition();
      localRate = state.targetBpm / state.trackBpm;
      invoke('set_playback_rate', { deck: id, rate: localRate });
    },

    async loadTrack(data: LoadableTrack) {
      bandsReadyUnlisten?.();
      bandsReadyUnlisten = null;
      loadGeneration++;
      state.loading = true;
      state.waveformLoading = true;
      if (state.loopPlaying) {
        await invoke('stop', { deck: id });
        state.loopPlaying = false;
      }
      state.cueing = false;
      state.nudging = null;
      state.loopRegion = null;
      state.loopActive = false;
      state.trackData = null;
      positionCache = 0;

      onBeatOffsetChangeCb = data.onBeatOffsetChange;

      const info = await invoke<TrackData>('load_track', {
        deck: id,
        path: data.path,
        analyze: false,
        beatOffsetSec: data.beatOffset
      });

      state.trackName = data.name;
      state.trackData = info;
      state.coverArt = info.coverArt ?? null;
      state.trackLoaded = true;
      state.loading = false;
      state.loadedPath = data.path;

      state.trackBpm = data.bpm;
      state.beatOffset = data.beatOffset;
      state.cuePoint = data.beatOffset;
      state.targetBpm = data.bpm;
      state.pitchOffset = 0;
      positionCache = data.beatOffset;
      clockAtPlay = performance.now();
      localRate = 1.0;
      await invoke('set_playback_rate', { deck: id, rate: 1.0 });
      invoke('set_beat_grid', { deck: id, bpm: data.bpm, beatOffsetSec: data.beatOffset });

      // Spectral bands are computed in the background by Rust. Listen for
      // bands-ready, then fetch both the low-rate overview and the dense
      // LOD once bands are available. Fetched in parallel: the overview
      // lands first (smaller), the dense LOD follows once its pass through
      // the bands buffers completes.
      const overviewPoints = Math.min(2000, Math.max(256, Math.ceil(info.duration * 4)));
      const densePoints = Math.max(256, Math.ceil(info.duration * DENSE_LOD_PTS_PER_SEC));
      const gen = loadGeneration;
      const unlisten = await listen<string>('bands-ready', async (event) => {
        if (event.payload !== id) return;
        bandsReadyUnlisten = null;
        setTimeout(unlisten, 0);
        try {
          const result = await state.getSpectralWaveformRegion(0, info.duration, overviewPoints);
          if (loadGeneration !== gen) return;
          state.fullSpectralData = new Float32Array(result);
          state.waveformLoading = false;
          fetchDenseLodChunked(gen, info.duration, densePoints);
        } catch {
          // spectral fetch failed; waveform will remain blank but deck is playable
          state.waveformLoading = false;
        }
      });
      bandsReadyUnlisten = unlisten;
    },

    setBeatOffset(sec: number) {
      state.beatOffset = sec;
      onBeatOffsetChangeCb?.(sec);
      if (state.trackBpm !== null) {
        invoke('set_beat_grid', { deck: id, bpm: state.trackBpm, beatOffsetSec: sec });
      }
    },

    moveLoopRegion(startSec: number) {
      if (!state.loopRegion) return;
      const dur = state.loopRegion.endSec - state.loopRegion.startSec;
      const endSec = startSec + dur;
      state.loopRegion = { ...state.loopRegion, startSec, endSec };
      invoke('set_loop_region', { deck: id, startSec, endSec });
    },

    async setLoopIn() {
      if (!state.trackLoaded) return;
      syncPosition();
      const payload = await invoke<DeckSyncPayload>('set_loop_in', { deck: id });
      applyDeckState(payload);
    },

    async setLoopOut() {
      if (!state.trackLoaded || state.trackBpm === null) return;
      const r = await invoke<{
        startSec: number;
        endSec: number;
        beats: number;
        seekToSec: number | null;
      } | null>('set_loop_out', { deck: id });
      if (!r) return;
      state.loopRegion = { startSec: r.startSec, endSec: r.endSec, beats: r.beats };
      state.loopActive = true;
      if (r.seekToSec !== null) {
        positionCache = r.seekToSec;
        clockAtPlay = performance.now();
      }
    },

    async exitLoop() {
      if (!state.loopActive) return;
      syncPosition();
      const payload = await invoke<DeckSyncPayload>('set_loop_active', { deck: id, active: false });
      applyDeckState(payload);
    },

    async reloop() {
      if (!state.loopRegion) return;
      positionCache = state.loopRegion.startSec;
      clockAtPlay = performance.now();
      const payload = await invoke<DeckSyncPayload>('set_reloop', { deck: id });
      applyDeckState(payload);
    },

    async togglePlay() {
      const payload = await invoke<DeckSyncPayload>('toggle_play', { deck: id });
      applyDeckState(payload);
    },

    async cueStart() {
      const payload = await invoke<DeckSyncPayload>('press_cue', { deck: id });
      applyDeckState(payload);
    },

    async cueEnd() {
      const payload = await invoke<DeckSyncPayload>('release_cue', { deck: id });
      applyDeckState(payload);
    },

    async setCueAndStop() {
      const payload = await invoke<DeckSyncPayload>('set_cue_and_stop', { deck: id });
      applyDeckState(payload);
    },

    async seekTo(sec: number) {
      const clamped = Math.max(0, sec);
      positionCache = clamped;
      clockAtPlay = performance.now();
      const payload = await invoke<DeckSyncPayload>('seek', { deck: id, sec: clamped });
      applyDeckState(payload);
    },

    getPlayheadPosition(): number {
      return interpolatedPosition();
    },

    toggleQuantized() {
      state.quantized = !state.quantized;
      invoke('set_quantize', { deck: id, quantize: state.quantized });
    },

    setEq(band: 'low' | 'mid' | 'high', db: number) {
      const clamped = Math.max(EQ_MIN_DB, Math.min(EQ_MAX_DB, db));
      state.eq[band] = clamped;
      invoke('set_eq', { deck: id, band, db: clamped });
    },

    async nudgeStart(direction: 'back' | 'forward') {
      if (!state.trackLoaded) return;
      state.nudging = direction;
      const nudgePct = useSettingsStore().nudgeSensitivity;
      const offset = direction === 'forward' ? nudgePct : -nudgePct;
      const result = await invoke<{ positionSec: number; effectiveRate: number }>('set_nudge', {
        deck: id,
        percent: offset
      });
      positionCache = result.positionSec;
      clockAtPlay = performance.now();
      localRate = result.effectiveRate;
    },

    async nudgeEnd() {
      state.nudging = null;
      const result = await invoke<{ positionSec: number; effectiveRate: number }>('set_nudge', {
        deck: id,
        percent: 0
      });
      positionCache = result.positionSec;
      clockAtPlay = performance.now();
      localRate = result.effectiveRate;
    },

    get playing(): boolean {
      return state.loopPlaying && !state.cueing;
    },

    getSpectralWaveformRegion(
      startSec: number,
      endSec: number,
      numPoints: number
    ): Promise<ArrayBuffer> {
      return invoke<ArrayBuffer>('get_spectral_waveform_region', {
        deck: id,
        startSec,
        endSec,
        numPoints
      });
    },

    returnToCue() {
      syncPosition();
      positionCache = state.cuePoint;
      state.loopPlaying = false;
      state.cueing = false;
    },

    async ejectTrack() {
      bandsReadyUnlisten?.();
      bandsReadyUnlisten = null;
      loadGeneration++;
      state.loading = false;
      state.waveformLoading = false;
      state.loopPlaying = false;
      state.cueing = false;
      state.nudging = null;
      state.loopRegion = null;
      state.loopActive = false;
      state.trackData = null;
      state.fullSpectralData = null;
      state.denseSpectralData = null;
      state.denseSpectralRate = 0;
      state.coverArt = null;
      state.trackName = '';
      state.trackLoaded = false;
      state.loadedPath = null;
      state.trackBpm = null;
      state.targetBpm = null;
      state.pitchOffset = 0;
      state.cuePoint = 0;
      state.beatOffset = 0;
      positionCache = 0;
      clockAtPlay = 0;
      localRate = 1.0;
      onBeatOffsetChangeCb = null;
      await invoke('eject_track', { deck: id });
    },

    async stop() {
      if (!state.loopPlaying) return;
      await invoke('stop', { deck: id });
      state.loopPlaying = false;
      state.cueing = false;
    },

    async destroy() {
      bandsReadyUnlisten?.();
      try {
        await invoke('stop', { deck: id });
      } catch {
        // ignore stop errors on teardown
      }
    }
  });

  return state;
}

export type Deck = ReturnType<typeof createDeck>;

export const useDecksStore = defineStore('decks', () => {
  const deckA = createDeck('A', '#3b82f6', 'DECK A');
  const deckB = createDeck('B', '#f97316', 'DECK B');
  const deckC = createDeck('C', '#208043', 'DECK C');
  const deckD = createDeck('D', '#d631b0', 'DECK D');
  const deckE = createDeck('E', '#a855f7', 'EDIT');

  const decks: Record<DeckId, ReturnType<typeof createDeck>> = {
    A: deckA,
    B: deckB,
    C: deckC,
    D: deckD,
    E: deckE
  };
  const editMode = ref(false);

  const anyDeckActive = computed(
    () =>
      deckA.loopPlaying ||
      deckA.cueing ||
      deckB.loopPlaying ||
      deckB.cueing ||
      deckC.loopPlaying ||
      deckC.cueing ||
      deckD.loopPlaying ||
      deckD.cueing ||
      deckE.loopPlaying ||
      deckE.cueing
  );

  listen<string>('track-ended', (event) => {
    const deck = decks[event.payload as DeckId];
    if (!deck) return;
    deck.returnToCue();
  });

  function tryToggleEditMode(): boolean {
    if (editMode.value) {
      editMode.value = false;
      return true;
    }
    if (deckA.loopPlaying || deckB.loopPlaying || deckC.loopPlaying || deckD.loopPlaying) {
      return false;
    }
    editMode.value = true;
    return true;
  }

  async function enterEditMode() {
    await Promise.all(
      DECKS_DISPOSITION.filter((deckId) => decks[deckId].loopPlaying).map((deckId) =>
        decks[deckId].stop()
      )
    );
    editMode.value = true;
  }

  function exitEditMode() {
    editMode.value = false;
  }

  function bestAvailableDeck(): DeckId | null {
    if (editMode.value) return 'E';
    return (
      DECKS_DISPOSITION.find((id) => !decks[id].trackLoaded) ??
      DECKS_DISPOSITION.find((id) => !decks[id].loopPlaying) ??
      null
    );
  }

  function destroy() {
    deckA.destroy();
    deckB.destroy();
    deckC.destroy();
    deckD.destroy();
    deckE.destroy();
  }

  return {
    deckA,
    deckB,
    deckC,
    deckD,
    deckE,
    decks,
    editMode,
    anyDeckActive,
    bestAvailableDeck,
    enterEditMode,
    exitEditMode,
    tryToggleEditMode,
    destroy
  };
});
