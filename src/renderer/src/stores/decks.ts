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
        analyze: false
      });

      state.trackName = data.name;
      state.trackData = info;
      state.coverArt = info.coverArt ?? null;
      state.trackLoaded = true;
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
      await invoke('seek', { deck: id, sec: data.beatOffset });
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
          state.loading = false;
          fetchDenseLodChunked(gen, info.duration, densePoints).catch(() => {});
        } catch {
          if (loadGeneration === gen) state.loading = false;
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
      const cueSec = await invoke<number>('set_loop_in', { deck: id, quantize: state.quantized });
      state.cuePoint = cueSec;
      state.loopActive = false;
      state.loopRegion = null;
    },

    async setLoopOut() {
      if (!state.trackLoaded || state.trackBpm === null) return;
      const r = await invoke<{
        start_sec: number;
        end_sec: number;
        beats: number;
        seek_to_sec: number | null;
      } | null>('set_loop_out', {
        deck: id,
        quantize: state.quantized,
        cuePointSec: state.cuePoint
      });
      if (!r) return;
      state.loopRegion = { startSec: r.start_sec, endSec: r.end_sec, beats: r.beats };
      state.loopActive = true;
      if (r.seek_to_sec !== null) {
        positionCache = r.seek_to_sec;
        clockAtPlay = performance.now();
      }
    },

    exitLoop() {
      if (!state.loopActive) return;
      syncPosition();
      state.loopActive = false;
      invoke('set_loop_active', { deck: id, active: false });
    },

    reloop() {
      if (!state.loopRegion) return;
      positionCache = state.loopRegion.startSec;
      clockAtPlay = performance.now();
      invoke('set_reloop', { deck: id });
      if (state.loopPlaying) {
        state.loopActive = true;
      }
    },

    async togglePlay() {
      if (!state.trackLoaded) return;
      if (state.cueing) {
        state.cueing = false;
        return;
      }
      if (state.loopPlaying) {
        syncPosition();
        await invoke('stop', { deck: id });
        state.loopPlaying = false;
      } else {
        await invoke('play', { deck: id });
        state.loopPlaying = true;
        clockAtPlay = performance.now();
      }
    },

    async cueStart() {
      if (!state.trackLoaded || state.cueing || state.loopPlaying) return;
      if (Math.abs(positionCache - state.cuePoint) > 0.001) {
        state.cuePoint = positionCache;
        if (state.loopRegion) {
          state.loopRegion = null;
          state.loopActive = false;
          invoke('clear_loop_region', { deck: id });
        }
        return;
      }
      state.cueing = true;
      state.loopPlaying = true;
      await invoke('play', { deck: id, fromSec: state.cuePoint });
      clockAtPlay = performance.now();
    },

    async cueEnd() {
      if (!state.cueing) return;
      state.cueing = false;
      state.loopPlaying = false;
      await invoke('stop', { deck: id });
      positionCache = state.cuePoint;
      clockAtPlay = performance.now();
      await invoke('seek', { deck: id, sec: state.cuePoint });
    },

    async setCueAndStop() {
      if (!state.trackLoaded || !state.loopPlaying || state.cueing) return;
      syncPosition();
      const pos = positionCache;
      state.cuePoint = pos;
      state.loopPlaying = false;
      await invoke('stop', { deck: id });
      positionCache = pos;
      clockAtPlay = performance.now();
      await invoke('seek', { deck: id, sec: pos });
    },

    async stopAtCue() {
      if (!state.loopPlaying || state.cueing) return;
      await invoke('stop', { deck: id });
      state.loopPlaying = false;
      positionCache = state.cuePoint;
      clockAtPlay = performance.now();
      await invoke('seek', { deck: id, sec: state.cuePoint });
    },

    seekTo(sec: number) {
      if (state.loopActive) {
        state.loopActive = false;
        state.loopRegion = null;
        invoke('set_loop_active', { deck: id, active: false });
      }
      const clamped = Math.max(0, sec);
      positionCache = clamped;
      clockAtPlay = performance.now();
      invoke('seek', { deck: id, sec: clamped });
    },

    getPlayheadPosition(): number {
      return interpolatedPosition();
    },

    toggleQuantized() {
      state.quantized = !state.quantized;
    },

    setEq(band: 'low' | 'mid' | 'high', db: number) {
      const clamped = Math.max(EQ_MIN_DB, Math.min(EQ_MAX_DB, db));
      state.eq[band] = clamped;
      invoke('set_eq', { deck: id, band, db: clamped });
    },

    nudgeStart(direction: 'back' | 'forward') {
      if (!state.trackLoaded) return;
      state.nudging = direction;
      const nudgePct = useSettingsStore().nudgeSensitivity;
      const offset = direction === 'forward' ? nudgePct : -nudgePct;
      syncPosition();
      const baseRate =
        state.targetBpm !== null && state.trackBpm !== null
          ? state.targetBpm / state.trackBpm
          : 1.0;
      localRate = baseRate * (1 + offset / 100);
      invoke('set_nudge', { deck: id, percent: offset });
    },

    nudgeEnd() {
      state.nudging = null;
      syncPosition();
      localRate =
        state.targetBpm !== null && state.trackBpm !== null
          ? state.targetBpm / state.trackBpm
          : 1.0;
      invoke('set_nudge', { deck: id, percent: 0 });
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

    naturallyEnded() {
      syncPosition();
      if (state.trackData) positionCache = state.trackData.duration;
      state.loopPlaying = false;
      state.cueing = false;
    },

    async ejectTrack() {
      bandsReadyUnlisten?.();
      bandsReadyUnlisten = null;
      loadGeneration++;
      state.loading = false;
      if (state.loopPlaying) {
        await invoke('stop', { deck: id });
        state.loopPlaying = false;
      }
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
    },

    destroy() {
      bandsReadyUnlisten?.();
      invoke('stop', { deck: id }).catch(() => {});
    }
  });

  return state;
}

export type Deck = ReturnType<typeof createDeck>;

export const useDecksStore = defineStore('decks', () => {
  const deckA = createDeck('A', '#3b82f6', 'Deck A');
  const deckB = createDeck('B', '#f97316', 'Deck B');
  const deckC = createDeck('C', '#208043', 'Deck C');
  const deckD = createDeck('D', '#d631b0', 'Deck D');
  const deckE = createDeck('E', '#a855f7', 'Edit');

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
    deck.naturallyEnded();
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

  function enterEditMode() {
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
