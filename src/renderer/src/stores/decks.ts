import { defineStore } from 'pinia';
import { reactive, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type DeckId = 'A' | 'B' | 'E';

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
};

export type LoadableTrack = {
  path: string;
  name: string;
  bpm: number;
  silenceEnd: number;
  beatOffset: number;
  onBeatOffsetChange: (sec: number) => void;
};

export const PITCH_RANGE = 10;

const NUDGE_PERCENT = 4;
export const EQ_MIN_DB = -26;
export const EQ_MAX_DB = 6;

// Dense LOD points-per-second. Sized to comfortably satisfy zoom >= ~5s on
// typical canvases (1000-2000px). For a 3-minute track this is ~650 KB
// of Float32 data; for 10 minutes ~2.1 MB. Anything zoomed deeper than the
// rate can cover (sub-second zoom levels) falls back to an on-demand fetch.
const DENSE_LOD_PTS_PER_SEC = 250;

function quantizeToBeat(sec: number, bpm: number, beatOffset: number): number {
  if (bpm <= 0) return sec;
  const beatDur = 60 / bpm;
  const beatIndex = Math.round((sec - beatOffset) / beatDur);
  return beatOffset + beatIndex * beatDur;
}

function createDeck(id: DeckId, accent: string) {
  let positionCache = 0;
  let clockAtPlay = 0; // performance.now() when playback started or position was last anchored
  let localRate = 1.0; // effective playback rate (pitch + nudge) for interpolation
  let onBeatOffsetChangeCb: ((sec: number) => void) | null = null;
  let bandsReadyUnlisten: (() => void) | null = null;

  // Re-anchor positionCache to now so rate changes don't cause a position jump.
  function syncPosition() {
    if (state.loopPlaying) {
      positionCache += ((performance.now() - clockAtPlay) / 1000) * localRate;
      clockAtPlay = performance.now();
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

    trackName: '',
    trackLoaded: false,
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
    loopPlaying: false,
    loopRegion: null as LoopRegion | null,
    loopActive: false,

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
      const minBpm = state.trackBpm * (1 - PITCH_RANGE / 100);
      const maxBpm = state.trackBpm * (1 + PITCH_RANGE / 100);
      const clamped = Math.max(minBpm, Math.min(maxBpm, value));
      state.targetBpm = clamped;
      state.pitchOffset = (clamped / state.trackBpm - 1) * 100;
      syncPosition();
      localRate = clamped / state.trackBpm;
      invoke('set_playback_rate', { deck: id, rate: localRate });
    },

    setPitchOffset(pct: number) {
      if (state.trackBpm === null) return;
      state.pitchOffset = Math.max(-PITCH_RANGE, Math.min(PITCH_RANGE, pct));
      state.targetBpm = state.trackBpm * (1 + state.pitchOffset / 100);
      syncPosition();
      localRate = state.targetBpm / state.trackBpm;
      invoke('set_playback_rate', { deck: id, rate: localRate });
    },

    async loadTrack(data: LoadableTrack) {
      bandsReadyUnlisten?.();
      bandsReadyUnlisten = null;
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
      positionCache = 0;

      onBeatOffsetChangeCb = data.onBeatOffsetChange;

      const info = await invoke<TrackData>('load_track', {
        deck: id,
        path: data.path,
        analyze: false
      });

      state.trackName = data.name;
      state.trackData = info;
      state.trackLoaded = true;

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

      // Spectral bands are computed in the background by Rust. Listen for
      // bands-ready, then fetch both the low-rate overview and the dense
      // LOD once bands are available. Fetched in parallel: the overview
      // lands first (smaller), the dense LOD follows once its pass through
      // the bands buffers completes.
      const overviewPoints = Math.min(2000, Math.max(256, Math.ceil(info.duration * 4)));
      const densePoints = Math.max(256, Math.ceil(info.duration * DENSE_LOD_PTS_PER_SEC));
      const unlisten = await listen<string>('bands-ready', (event) => {
        if (event.payload !== id) return;
        bandsReadyUnlisten = null;
        unlisten();
        invoke<number[]>('get_spectral_waveform_region', {
          deck: id,
          startSec: 0,
          endSec: info.duration,
          numPoints: overviewPoints
        })
          .then((result) => {
            state.fullSpectralData = new Float32Array(result);
          })
          .catch(() => {});
        invoke<number[]>('get_spectral_waveform_region', {
          deck: id,
          startSec: 0,
          endSec: info.duration,
          numPoints: densePoints
        })
          .then((result) => {
            state.denseSpectralData = new Float32Array(result);
            state.denseSpectralRate = densePoints / info.duration;
          })
          .catch(() => {});
      });
      bandsReadyUnlisten = unlisten;
    },

    setBeatOffset(sec: number) {
      state.beatOffset = sec;
      onBeatOffsetChangeCb?.(sec);
    },

    setLoopRegion(region: LoopRegion) {
      state.loopRegion = region;
      invoke('set_loop_region', {
        deck: id,
        startSec: region.startSec,
        endSec: region.endSec
      });
    },

    moveLoopRegion(startSec: number) {
      if (!state.loopRegion) return;
      const dur = state.loopRegion.endSec - state.loopRegion.startSec;
      const region = { ...state.loopRegion, startSec, endSec: startSec + dur };
      state.loopRegion = region;
      invoke('set_loop_region', { deck: id, startSec, endSec: startSec + dur });
    },

    setLoopIn() {
      if (!state.trackLoaded || state.trackBpm === null) return;
      const inSec = quantizeToBeat(interpolatedPosition(), state.trackBpm, state.beatOffset);
      const barDur = (4 * 60) / state.trackBpm;
      const outSec =
        state.loopRegion && state.loopRegion.endSec > inSec + 0.01
          ? state.loopRegion.endSec
          : inSec + barDur;
      const beats = Math.round(((outSec - inSec) * state.trackBpm) / 60);
      if (state.loopActive) {
        state.loopActive = false;
        invoke('set_loop_active', { deck: id, active: false });
      }
      state.setLoopRegion({ startSec: inSec, endSec: outSec, beats });
    },

    setLoopOut() {
      if (!state.trackLoaded || state.trackBpm === null) return;
      const barDur = (4 * 60) / state.trackBpm;
      const outSec = quantizeToBeat(interpolatedPosition(), state.trackBpm, state.beatOffset);
      const inSec = state.loopRegion ? state.loopRegion.startSec : outSec - barDur;
      if (outSec <= inSec) return;
      const beats = Math.round(((outSec - inSec) * state.trackBpm) / 60);
      state.setLoopRegion({ startSec: inSec, endSec: outSec, beats });
      state.loopActive = true;
      invoke('set_loop_active', { deck: id, active: true });
    },

    reloopOrExit() {
      if (!state.loopRegion) return;
      if (state.loopActive) {
        state.loopActive = false;
        invoke('set_loop_active', { deck: id, active: false });
      } else {
        invoke('set_reloop', { deck: id });
        if (state.loopPlaying) {
          state.loopActive = true;
        }
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
      const clamped = Math.max(0, sec);
      positionCache = clamped;
      clockAtPlay = performance.now();
      invoke('seek', { deck: id, sec: clamped });
    },

    getPlayheadPosition(): number {
      return interpolatedPosition();
    },

    setEq(band: 'low' | 'mid' | 'high', db: number) {
      const clamped = Math.max(EQ_MIN_DB, Math.min(EQ_MAX_DB, db));
      state.eq[band] = clamped;
      invoke('set_eq', { deck: id, band, db: clamped });
    },

    nudgeStart(direction: 'back' | 'forward') {
      if (!state.trackLoaded) return;
      state.nudging = direction;
      const offset = direction === 'forward' ? NUDGE_PERCENT : -NUDGE_PERCENT;
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
    ): Promise<number[]> {
      return invoke<number[]>('get_spectral_waveform_region', {
        deck: id,
        startSec,
        endSec,
        numPoints
      });
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
  const deckA = createDeck('A', '#3b82f6');
  const deckB = createDeck('B', '#f97316');
  const deckE = createDeck('E', '#a855f7');

  const decks: Record<DeckId, ReturnType<typeof createDeck>> = { A: deckA, B: deckB, E: deckE };
  const editMode = ref(false);

  function destroy() {
    deckA.destroy();
    deckB.destroy();
    deckE.destroy();
  }

  return {
    deckA,
    deckB,
    deckE,
    decks,
    editMode,
    destroy
  };
});
