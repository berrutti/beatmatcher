import { defineStore } from 'pinia';
import { reactive } from 'vue';
import { AudioEngine } from '@renderer/audio/AudioEngine';
import type { LoopRegion } from '@renderer/audio/AudioEngine';
import { detectBpm, detectSilenceEnd } from '@renderer/audio/bpmDetect';

export type DeckId = 'A' | 'B';
export type DeckMode = 'edit' | 'play';

const NUDGE_PERCENT = 4;
const START_LATENCY = 0.05;
export const PITCH_RANGE = 10;

type SavedTrack = {
  trackBpm: number;
  beatOffset: number;
  cuePoint?: number;
  loopRegion?: { startSec: number; endSec: number; beats: number };
};

const STORAGE_KEY = 'beatmatcher:track-regions';

function loadSavedTracks(): Record<string, SavedTrack> {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}');
  } catch {
    return {};
  }
}

function saveTrack(filename: string, data: SavedTrack) {
  const all = loadSavedTracks();
  all[filename] = data;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
}

function getSavedTrack(filename: string): SavedTrack | null {
  const raw = loadSavedTracks()[filename] as (SavedTrack & { startSec?: number; endSec?: number; beats?: number; detectedBpm?: number }) | undefined;
  if (!raw) return null;
  // Current format
  if (raw.trackBpm && raw.beatOffset !== undefined && !raw.startSec) return raw as SavedTrack;
  // Migrate old format: { startSec, endSec, beats, trackBpm/detectedBpm, beatOffset }
  const bpm = raw.trackBpm || raw.detectedBpm;
  if (!bpm) return null;
  const beatOffset = raw.beatOffset ?? raw.startSec ?? 0;
  const result: SavedTrack = { trackBpm: bpm, beatOffset };
  if (raw.startSec !== undefined && raw.endSec !== undefined && raw.beats) {
    result.loopRegion = { startSec: raw.startSec, endSec: raw.endSec, beats: raw.beats };
  }
  return result;
}

function quantizeToBeat(sec: number, bpm: number, beatOffset: number): number {
  if (bpm <= 0) return sec;
  const beatDur = 60 / bpm;
  const beatIndex = Math.round((sec - beatOffset) / beatDur);
  return beatOffset + beatIndex * beatDur;
}

function createDeck(id: DeckId, accent: string) {
  const audioCtx = new AudioContext();
  const loop = new AudioEngine(audioCtx);

  let currentFilename = '';

  const state = reactive({
    id,
    accent,
    mode: 'play' as DeckMode,

    trackName: '',
    trackLoaded: false,
    buffer: null as AudioBuffer | null,
    detecting: false,
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
      return loop.trackPosition;
    },
    get phase(): number {
      return loop.phase;
    },

    setTargetBpm(value: number) {
      if (state.trackBpm === null) return;
      const minBpm = state.trackBpm * (1 - PITCH_RANGE / 100);
      const maxBpm = state.trackBpm * (1 + PITCH_RANGE / 100);
      const clamped = Math.max(minBpm, Math.min(maxBpm, value));
      state.targetBpm = clamped;
      state.pitchOffset = (clamped / state.trackBpm - 1) * 100;
      loop.targetBpm = clamped;
    },

    setPitchOffset(pct: number) {
      if (state.trackBpm === null) return;
      state.pitchOffset = Math.max(-PITCH_RANGE, Math.min(PITCH_RANGE, pct));
      state.targetBpm = state.trackBpm * (1 + state.pitchOffset / 100);
      loop.targetBpm = state.targetBpm;
    },

    async loadTrack(file: File, onNeedBpmInput: () => void) {
      if (state.loopPlaying) {
        loop.stop();
        state.loopPlaying = false;
      }
      state.cueing = false;
      state.nudging = null;
      loop.setNudge(0);
      state.loopRegion = null;
      state.loopActive = false;
      state.buffer = null;

      await loop.loadFile(file);
      state.buffer = loop.bufferData;
      currentFilename = file.name;
      state.trackName = file.name;
      state.trackLoaded = true;

      const saved = getSavedTrack(file.name);
      if (saved) {
        state.trackBpm = saved.trackBpm;
        state.beatOffset = saved.beatOffset;
        state.cuePoint = saved.cuePoint ?? saved.beatOffset;
        loop.trackBpm = saved.trackBpm;
        loop.beatOffset = saved.beatOffset;
        loop.seekTo(state.cuePoint);
        state.targetBpm = saved.trackBpm;
        state.pitchOffset = 0;
        loop.targetBpm = saved.trackBpm;
        if (saved.loopRegion) {
          state.loopRegion = saved.loopRegion;
          loop.setRegion(saved.loopRegion);
        }
        state.mode = 'edit';
        return;
      }

      state.detecting = true;
      let detectedBpm = 0;
      let silenceEnd = 0;
      if (state.buffer) {
        const result = await detectBpm(state.buffer);
        if (result.bpm > 0) detectedBpm = result.bpm;
        silenceEnd = detectSilenceEnd(state.buffer);
      }
      state.detecting = false;

      if (detectedBpm > 0) {
        state.setTrackBpm(detectedBpm, silenceEnd);
        state.mode = 'edit';
      } else {
        onNeedBpmInput();
      }
    },

    setEditMode() {
      state.mode = 'edit';
    },

    setPlayMode() {
      state.mode = 'play';
    },

    // Sets the track's native BPM and beat grid offset. No loop region is created
    // automatically — the grid alone is enough to show beat markers on the waveform.
    setTrackBpm(bpm: number, beatOffsetSec = state.beatOffset) {
      state.trackBpm = bpm;
      state.beatOffset = beatOffsetSec;
      state.cuePoint = beatOffsetSec;
      loop.trackBpm = bpm;
      loop.beatOffset = beatOffsetSec;
      loop.seekTo(beatOffsetSec);
      state.targetBpm = bpm;
      state.pitchOffset = 0;
      loop.targetBpm = bpm;
      if (currentFilename) {
        saveTrack(currentFilename, {
          trackBpm: bpm,
          beatOffset: beatOffsetSec,
          cuePoint: beatOffsetSec,
          loopRegion: state.loopRegion
            ? { startSec: state.loopRegion.startSec, endSec: state.loopRegion.endSec, beats: state.loopRegion.beats }
            : undefined
        });
      }
    },

    setBeatOffset(sec: number) {
      state.beatOffset = sec;
      loop.beatOffset = sec;
      if (currentFilename && state.trackBpm !== null) {
        saveTrack(currentFilename, {
          trackBpm: state.trackBpm,
          beatOffset: sec,
          cuePoint: state.cuePoint,
          loopRegion: state.loopRegion
            ? { startSec: state.loopRegion.startSec, endSec: state.loopRegion.endSec, beats: state.loopRegion.beats }
            : undefined
        });
      }
    },

    setLoopRegion(region: LoopRegion) {
      state.loopRegion = region;
      loop.setRegion(region);
      if (currentFilename && state.trackBpm !== null) {
        saveTrack(currentFilename, {
          trackBpm: state.trackBpm,
          beatOffset: state.beatOffset,
          cuePoint: state.cuePoint,
          loopRegion: { startSec: region.startSec, endSec: region.endSec, beats: region.beats }
        });
      }
    },

    moveLoopRegion(startSec: number) {
      if (!state.loopRegion) return;
      const dur = state.loopRegion.endSec - state.loopRegion.startSec;
      const region = { ...state.loopRegion, startSec, endSec: startSec + dur };
      state.loopRegion = region;
      loop.setRegion(region);
      if (currentFilename && state.trackBpm !== null) {
        saveTrack(currentFilename, {
          trackBpm: state.trackBpm,
          beatOffset: state.beatOffset,
          cuePoint: state.cuePoint,
          loopRegion: { startSec: region.startSec, endSec: region.endSec, beats: region.beats }
        });
      }
    },

    // Set loop in-point at the current position, quantized to the nearest beat.
    setLoopIn() {
      if (!state.trackLoaded || state.trackBpm === null) return;
      const inSec = quantizeToBeat(loop.position, state.trackBpm, state.beatOffset);
      const barDur = (4 * 60) / state.trackBpm;
      const outSec = state.loopRegion && state.loopRegion.endSec > inSec + 0.01
        ? state.loopRegion.endSec
        : inSec + barDur;
      const beats = Math.round((outSec - inSec) * state.trackBpm / 60);
      if (state.loopActive) {
        state.loopActive = false;
        loop.loopActive = false;
      }
      state.setLoopRegion({ startSec: inSec, endSec: outSec, beats });
    },

    // Set loop out-point at the current position, quantized to the nearest beat, and activate the loop.
    setLoopOut() {
      if (!state.trackLoaded || state.trackBpm === null) return;
      const barDur = (4 * 60) / state.trackBpm;
      const outSec = quantizeToBeat(loop.position, state.trackBpm, state.beatOffset);
      const inSec = state.loopRegion
        ? state.loopRegion.startSec
        : outSec - barDur;
      if (outSec <= inSec) return;
      const beats = Math.round((outSec - inSec) * state.trackBpm / 60);
      state.setLoopRegion({ startSec: inSec, endSec: outSec, beats });
      state.loopActive = true;
      loop.loopActive = true;
    },

    reloopOrExit() {
      if (!state.loopRegion) return;
      if (state.loopActive) {
        // EXIT: deactivate loop, continue playing from current position
        state.loopActive = false;
        loop.loopActive = false;
      } else {
        // RELOOP: if playing → jump to loop start and enter loop
        //         if stopped → park at loop start only
        loop.reloop();
        if (state.loopPlaying) {
          state.loopActive = true;
        }
      }
    },

    togglePlay() {
      if (!state.trackLoaded) return;
      if (state.cueing) {
        state.cueing = false;
        return;
      }
      if (state.loopPlaying) {
        loop.stop();
        state.loopPlaying = false;
      } else {
        loop.startAt(audioCtx.currentTime + START_LATENCY);
        state.loopPlaying = true;
      }
    },

    // Hold CUE while stopped: preview from cue point. Release returns to cue point.
    cueStart() {
      if (!state.trackLoaded) return;
      if (state.cueing || state.loopPlaying) return;
      state.cueing = true;
      loop.startAt(audioCtx.currentTime + START_LATENCY, state.cuePoint);
      state.loopPlaying = true;
    },

    cueEnd() {
      if (!state.cueing) return;
      state.cueing = false;
      loop.stop();
      state.loopPlaying = false;
      loop.seekTo(state.cuePoint);
    },

    // Press CUE while playing: set the cue point at the current position and stop there.
    setCueAndStop() {
      if (!state.trackLoaded || !state.loopPlaying || state.cueing) return;
      const pos = loop.trackPosition;
      if (pos === null) return;
      state.cuePoint = pos;
      loop.stop();
      state.loopPlaying = false;
      loop.seekTo(pos);
      if (currentFilename && state.trackBpm !== null) {
        saveTrack(currentFilename, {
          trackBpm: state.trackBpm,
          beatOffset: state.beatOffset,
          cuePoint: pos,
          loopRegion: state.loopRegion
            ? { startSec: state.loopRegion.startSec, endSec: state.loopRegion.endSec, beats: state.loopRegion.beats }
            : undefined
        });
      }
    },

    setEq(band: 'low' | 'mid' | 'high', db: number) {
      state.eq[band] = db;
      loop.setEq(band, db);
    },

    nudgeStart(direction: 'back' | 'forward') {
      if (!state.trackLoaded) return;
      state.nudging = direction;
      const offset = direction === 'forward' ? NUDGE_PERCENT : -NUDGE_PERCENT;
      loop.setNudge(offset);
    },

    nudgeEnd() {
      state.nudging = null;
      loop.setNudge(0);
    },

    get playing(): boolean {
      return state.loopPlaying && !state.cueing;
    },

    destroy() {
      loop.destroy();
      audioCtx.close();
    }
  });

  return state;
}

export type Deck = ReturnType<typeof createDeck>;

export const useDecksStore = defineStore('decks', () => {
  const deckA = createDeck('A', '#3b82f6');
  const deckB = createDeck('B', '#f97316');

  const decks: Record<DeckId, ReturnType<typeof createDeck>> = { A: deckA, B: deckB };

  function destroy() {
    deckA.destroy();
    deckB.destroy();
  }

  return { deckA, deckB, decks, destroy };
});
