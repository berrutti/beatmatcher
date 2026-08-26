import { defineStore } from 'pinia';
import { reactive, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { call } from '@renderer/tauriCommands';
import { listen } from '@tauri-apps/api/event';
import { useSettingsStore } from '@renderer/stores/settings';
import { currentBeat as coreCurrentBeat } from '@renderer/utils/sessionCore';
import { DECK_ACCENTS, type DeckId } from '@renderer/utils/types';

export const DECKS_DISPOSITION = ['C', 'A', 'B', 'D'] as const;

// The pair a two-deck mixer shows. C and D are the outer decks of the four-deck layout, so
// they are the ones that go, and the remaining two keep their disposition order.
export const TWO_DECK_DISPOSITION = ['A', 'B'] as const;

type LoopRegion = {
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
  bpm: number | null;
  silenceEnd: number;
  beatOffset: number;
  onBeatOffsetChange: (sec: number) => void;
};

// Dense LOD points-per-second. Sized to comfortably satisfy zoom >= ~5s on
// typical canvases (1000-2000px). For a 3-minute track this is ~650 KB
// of Float32 data. For 10 minutes ~2.1 MB. Anything zoomed deeper than the
// rate can cover (sub-second zoom levels) falls back to an on-demand fetch.
const DENSE_LOD_PTS_PER_SEC = 250;

type DeckSyncPayload = {
  isPlaying: boolean;
  isCueing: boolean;
  cuePointSec: number;
  positionSec: number;
  loopActive: boolean;
  loopRegionCleared: boolean;
  loopRegion: LoopRegion | null;
};

type TransportPush = DeckSyncPayload & { deck: DeckId };

type RatePush = { deck: DeckId; rate: number };

// The deck header shows BPM with two decimals. The audible rate is computed
// from the rounded value so display and playback always agree.
function roundBpm(bpm: number): number {
  return Math.round(bpm * 100) / 100;
}

// Everything a track puts on a deck. Ejecting applies this again rather than
// naming each field to clear, so a field added here cannot be forgotten there.
type DeckTrackState = {
  trackName: string;
  trackLoaded: boolean;
  loading: boolean;
  waveformLoading: boolean;
  loadedPath: string | null;
  trackData: TrackData | null;
  // Low-rate overview over the whole track, used by the overview strip and as a
  // first-paint fallback in WaveformDisplay while the dense LOD loads.
  fullSpectralData: Float32Array | null;
  // Higher-rate LOD over the whole track, sliced in JS for any zoom the rate can satisfy.
  // Deeper zooms fall back to on-demand fetches, see WaveformDisplay.
  denseSpectralData: Float32Array | null;
  denseSpectralRate: number;
  coverArt: string | null;
  loopPlaying: boolean;
  loopRegion: LoopRegion | null;
  loopActive: boolean;
  ejectPending: boolean;
  trackBpm: number | null;
  beatOffset: number;
  cuePoint: number;
  targetBpm: number | null;
  pitchOffset: number;
  nudging: 'back' | 'forward' | null;
  cueing: boolean;
};

function emptyDeck(): DeckTrackState {
  return {
    trackName: '',
    trackLoaded: false,
    loading: false,
    waveformLoading: false,
    loadedPath: null,
    trackData: null,
    fullSpectralData: null,
    denseSpectralData: null,
    denseSpectralRate: 0,
    coverArt: null,
    loopPlaying: false,
    loopRegion: null,
    loopActive: false,
    ejectPending: false,
    trackBpm: null,
    beatOffset: 0,
    cuePoint: 0,
    targetBpm: null,
    pitchOffset: 0,
    nudging: null,
    cueing: false
  };
}

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
    } else if (payload.loopRegion) {
      state.loopRegion = payload.loopRegion;
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
    ...emptyDeck(),
    // Not part of the empty deck: it is a setting, and survives the track.
    quantized: true,

    get trackPosition(): number | null {
      return state.loopPlaying ? interpolatedPosition() : null;
    },

    // Beat math lives in the engine, not the view. Deliberately not gated on
    // playback, so a paused deck keeps the ring frozen where the playhead sits.
    get beat(): number | null {
      if (state.trackBpm === null || !state.trackLoaded) return null;
      return coreCurrentBeat(interpolatedPosition(), state.beatOffset, state.trackBpm);
    },

    get phase(): number {
      if (state.trackBpm === null || !state.loopPlaying) return 0;
      const beats = coreCurrentBeat(interpolatedPosition(), state.beatOffset, state.trackBpm);
      return ((beats % 1) + 1) % 1;
    },

    get hasGrid(): boolean {
      return state.trackBpm !== null;
    },

    // Nudge is not included: it is a transient offset the engine applies.
    get rate(): number {
      if (state.trackBpm === null || state.targetBpm === null) {
        return 1 + state.pitchOffset / 100;
      }
      return state.targetBpm / state.trackBpm;
    },

    setTargetBpm(value: number) {
      if (state.trackBpm === null) return;
      const pitchRange = useSettingsStore().pitchRange;
      const minBpm = state.trackBpm * (1 - pitchRange / 100);
      const maxBpm = state.trackBpm * (1 + pitchRange / 100);
      // Rounding up at the top of the range would otherwise leave it.
      const rounded = roundBpm(Math.max(minBpm, Math.min(maxBpm, value)));
      const clamped = Math.max(minBpm, Math.min(maxBpm, rounded));
      state.targetBpm = clamped;
      state.pitchOffset = (clamped / state.trackBpm - 1) * 100;
      syncPosition();
      localRate = clamped / state.trackBpm;
      call('set_playback_rate', { deck: id, rate: localRate });
    },

    setTrackBpm(bpm: number) {
      state.trackBpm = bpm;
      state.targetBpm = bpm;
      state.pitchOffset = 0;
      syncPosition();
      localRate = 1.0;
      call('set_playback_rate', { deck: id, rate: 1.0 });
      call('set_beat_grid', { deck: id, bpm, beatOffsetSec: state.beatOffset });
    },

    async setPitchOffset(pct: number) {
      const pitchRange = useSettingsStore().pitchRange;
      state.pitchOffset = Math.max(-pitchRange, Math.min(pitchRange, pct));
      if (state.trackBpm === null) {
        const rate = await call('set_pitch_offset', { deck: id, percent: pct });
        syncPosition();
        localRate = rate;
        state.pitchOffset = (rate - 1) * 100;
        return;
      }
      const minBpm = state.trackBpm * (1 - pitchRange / 100);
      const maxBpm = state.trackBpm * (1 + pitchRange / 100);
      // The rate below comes from this bpm, and rounding can cross the range edge.
      const rounded = roundBpm(state.trackBpm * (1 + state.pitchOffset / 100));
      state.targetBpm = Math.max(minBpm, Math.min(maxBpm, rounded));
      syncPosition();
      localRate = state.targetBpm / state.trackBpm;
      call('set_playback_rate', { deck: id, rate: localRate });
    },

    async loadTrack(data: LoadableTrack) {
      bandsReadyUnlisten?.();
      bandsReadyUnlisten = null;
      loadGeneration++;
      state.loading = true;
      state.waveformLoading = true;
      if (state.loopPlaying) {
        await call('stop', { deck: id });
        state.loopPlaying = false;
      }
      state.cueing = false;
      state.nudging = null;
      state.loopRegion = null;
      state.loopActive = false;
      state.trackData = null;
      positionCache = 0;

      // Named before the decode, which takes long enough that a glance at the
      // deck in between reads the track that was there before. Not loaded until
      // it returns, so a swap shows the same pending state as an empty deck.
      state.trackName = data.name;
      state.coverArt = null;
      state.trackLoaded = false;

      onBeatOffsetChangeCb = data.onBeatOffsetChange;

      let info: TrackData;
      try {
        info = await invoke<TrackData>('load_track', {
          deck: id,
          path: data.path,
          analyze: false,
          beatOffsetSec: data.beatOffset
        });
      } catch (error) {
        // A deck left mid-load refuses every command for the rest of the session,
        // and would show the name of a track it does not hold.
        state.loading = false;
        state.waveformLoading = false;
        state.trackName = '';
        throw error;
      }

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
      await call('set_playback_rate', { deck: id, rate: 1.0 });
      call('set_beat_grid', { deck: id, bpm: data.bpm, beatOffsetSec: data.beatOffset });

      // Fetched in parallel once Rust says the bands are ready: the overview
      // lands first, the dense LOD after its pass through the band buffers.
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
          // spectral fetch failed. Waveform will remain blank but deck is playable
          state.waveformLoading = false;
        }
      });
      bandsReadyUnlisten = unlisten;
    },

    setBeatOffset(sec: number) {
      state.beatOffset = sec;
      onBeatOffsetChangeCb?.(sec);
      call('set_beat_grid', { deck: id, bpm: state.trackBpm, beatOffsetSec: sec });
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

    // Engine-originated, and deliberately does not invoke back: Rust never pushes a change
    // the UI made, so anything arriving here moved the engine without passing this store.
    applyEngineTransport(payload: DeckSyncPayload) {
      applyDeckState(payload);
    },

    // The engine owns the rate. Bpm and pitch offset are this store's derived
    // display of it, so they are recomputed here rather than invoked back.
    applyEngineRate(rate: number) {
      // Ahead of the grid check: the tempo fader is not gated on a grid, so a deck showing
      // --.- still has to interpolate at the rate the engine is actually playing.
      syncPosition();
      localRate = rate;
      state.pitchOffset = (rate - 1) * 100;
      if (state.trackBpm === null) return;
      state.targetBpm = roundBpm(state.trackBpm * rate);
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
      call('set_quantize', { deck: id, quantize: state.quantized });
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

    get acceptsCommands(): boolean {
      return !state.loading;
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
      positionCache = 0;
      clockAtPlay = 0;
      localRate = 1.0;
      onBeatOffsetChangeCb = null;
      Object.assign(state, emptyDeck());
      await call('eject_track', { deck: id });
    },

    // The confirmation lives here rather than in the button, so the eject key on
    // a controller passes the same guard the mouse does.
    async requestEject() {
      if (!state.trackLoaded) return;
      if (state.loopPlaying) {
        state.ejectPending = true;
        return;
      }
      await state.ejectTrack();
    },

    async confirmEject() {
      state.ejectPending = false;
      await state.ejectTrack();
    },

    cancelEject() {
      state.ejectPending = false;
    },

    async stop() {
      if (!state.loopPlaying) return;
      await call('stop', { deck: id });
      state.loopPlaying = false;
      state.cueing = false;
    },

    async destroy() {
      bandsReadyUnlisten?.();
      try {
        await call('stop', { deck: id });
      } catch {
        // ignore stop errors on teardown
      }
    }
  });

  return state;
}

export type Deck = ReturnType<typeof createDeck>;

export const useDecksStore = defineStore('decks', () => {
  const deckA = createDeck('A', DECK_ACCENTS.A, 'DECK A');
  const deckB = createDeck('B', DECK_ACCENTS.B, 'DECK B');
  const deckC = createDeck('C', DECK_ACCENTS.C, 'DECK C');
  const deckD = createDeck('D', DECK_ACCENTS.D, 'DECK D');
  const deckE = createDeck('E', DECK_ACCENTS.E, 'EDIT');

  const decks: Record<DeckId, ReturnType<typeof createDeck>> = {
    A: deckA,
    B: deckB,
    C: deckC,
    D: deckD,
    E: deckE
  };
  const anyDeckActive = computed(
    () =>
      deckA.loopPlaying ||
      deckA.cueing ||
      deckB.loopPlaying ||
      deckB.cueing ||
      deckC.loopPlaying ||
      deckC.cueing ||
      deckD.loopPlaying ||
      deckD.cueing
  );

  const anyDeckLoaded = computed(
    () =>
      deckA.loadedPath !== null ||
      deckB.loadedPath !== null ||
      deckC.loadedPath !== null ||
      deckD.loadedPath !== null
  );

  listen<string>('track-ended', (event) => {
    const deck = decks[event.payload as DeckId];
    if (!deck) return;
    deck.returnToCue();
  });

  // Rust forwards the press rather than flipping the flag: the store owns it,
  // and writing back through `set_quantize` is what lights the button.
  listen<string>('midi-quantize', (event) => {
    const found = DECKS_DISPOSITION.find((id) => id === event.payload);
    if (!found) return;
    decks[found].toggleQuantized();
  });

  listen<string>('midi-eject', async (event) => {
    const found = DECKS_DISPOSITION.find((id) => id === event.payload);
    if (!found) return;
    await decks[found].requestEject();
  });

  listen<TransportPush[]>('engine-transport', (event) => {
    for (const push of event.payload) {
      const deck = decks[push.deck];
      if (!deck) continue;
      deck.applyEngineTransport(push);
    }
  });

  listen<RatePush[]>('engine-rate', (event) => {
    for (const push of event.payload) {
      const deck = decks[push.deck];
      if (!deck) continue;
      deck.applyEngineRate(push.rate);
    }
  });

  async function ejectAll(): Promise<void> {
    await Promise.all(DECKS_DISPOSITION.map((id) => decks[id].ejectTrack()));
  }

  function bestAvailableDeck(inEditMode: boolean): DeckId | null {
    if (inEditMode) return 'E';
    return (
      DECKS_DISPOSITION.find((id) => !decks[id].trackLoaded) ??
      DECKS_DISPOSITION.find((id) => !decks[id].loopPlaying) ??
      null
    );
  }

  const settingsStore = useSettingsStore();

  watch(
    () => settingsStore.deckAccents,
    (accents) => {
      for (const [id, color] of Object.entries(accents)) {
        const deck = decks[id as DeckId];
        if (deck) deck.accent = color;
      }
    },
    { immediate: true }
  );

  function setDeckAccent(id: string, color: string): void {
    const deck = decks[id as DeckId];
    if (!deck) return;
    deck.accent = color;
    settingsStore.setDeckAccents({ ...settingsStore.deckAccents, [id]: color });
  }

  function resetDeckAccents(): void {
    for (const [id, color] of Object.entries(DECK_ACCENTS)) {
      const deck = decks[id as DeckId];
      if (deck) deck.accent = color;
    }
    settingsStore.setDeckAccents({});
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
    anyDeckActive,
    anyDeckLoaded,
    ejectAll,
    bestAvailableDeck,
    setDeckAccent,
    resetDeckAccents,
    destroy
  };
});
