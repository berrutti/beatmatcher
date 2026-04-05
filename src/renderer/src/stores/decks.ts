import { defineStore } from 'pinia';
import { reactive } from 'vue';
import { LoopEngine } from '@renderer/audio/LoopEngine';
import type { LoopRegion } from '@renderer/audio/LoopEngine';
import { detectBpm } from '@renderer/audio/bpmDetect';

export type DeckId = 'A' | 'B';
export type DeckMode = 'edit' | 'play';

const DEFAULT_BPM = 138;
const NUDGE_PERCENT = 4;
const START_LATENCY = 0.05;
export const PITCH_RANGE = 10;

type SavedRegion = { startSec: number; endSec: number; beats: 16 | 32; detectedBpm: number };
const STORAGE_KEY = 'beatmatcher:track-regions';

function loadSavedRegions(): Record<string, SavedRegion> {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}');
  } catch {
    return {};
  }
}

function saveRegion(filename: string, region: SavedRegion) {
  const all = loadSavedRegions();
  all[filename] = region;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
}

function getSavedRegion(filename: string): SavedRegion | null {
  const saved = loadSavedRegions()[filename];
  if (!saved || !saved.detectedBpm) return null;
  return saved;
}

function createDeck(id: DeckId, accent: string) {
  const audioCtx = new AudioContext();
  const loop = new LoopEngine(audioCtx);

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
    loopBeats: 16 as 16 | 32,

    inferredBpm: DEFAULT_BPM,
    targetBpm: DEFAULT_BPM,
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
      const minBpm = state.inferredBpm * (1 - PITCH_RANGE / 100);
      const maxBpm = state.inferredBpm * (1 + PITCH_RANGE / 100);
      state.targetBpm = Math.max(minBpm, Math.min(maxBpm, value));
      state.pitchOffset = (state.targetBpm / state.inferredBpm - 1) * 100;
      loop.targetBpm = state.targetBpm;
    },

    setPitchOffset(pct: number) {
      state.pitchOffset = Math.max(-PITCH_RANGE, Math.min(PITCH_RANGE, pct));
      state.targetBpm = state.inferredBpm * (1 + state.pitchOffset / 100);
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
      state.buffer = null;

      await loop.loadFile(file);
      state.buffer = loop.buffer_;
      currentFilename = file.name;
      state.trackName = file.name;
      state.trackLoaded = true;

      const saved = getSavedRegion(file.name);
      if (saved) {
        state.loopBeats = saved.beats;
        state.setLoopRegion(saved);
        return;
      }

      state.detecting = true;
      let detectedBpm = 0;
      if (state.buffer) {
        const result = await detectBpm(state.buffer);
        if (result.bpm > 0) detectedBpm = result.bpm;
      }
      state.detecting = false;

      if (detectedBpm > 0) {
        state.setTrackBpm(detectedBpm);
        state.mode = 'edit';
      } else {
        onNeedBpmInput();
      }
    },

    setTrackBpm(bpm: number) {
      const start = state.loopRegion?.startSec ?? 0;
      const dur = (state.loopBeats / bpm) * 60;
      state.setLoopRegion({ startSec: start, endSec: start + dur, beats: state.loopBeats });
    },

    setLoopRegion(region: LoopRegion) {
      state.loopRegion = region;
      loop.setRegion(region);
      state.inferredBpm = loop.inferredBpm;
      state.targetBpm = state.inferredBpm;
      state.pitchOffset = 0;
      loop.targetBpm = state.targetBpm;
      if (currentFilename)
        saveRegion(currentFilename, { ...region, detectedBpm: state.inferredBpm });
    },

    moveLoopRegion(startSec: number) {
      if (!state.loopRegion) return;
      const dur = state.loopRegion.endSec - state.loopRegion.startSec;
      const region = { ...state.loopRegion, startSec, endSec: startSec + dur };
      state.loopRegion = region;
      loop.setRegion(region);
      if (currentFilename)
        saveRegion(currentFilename, { ...region, detectedBpm: state.inferredBpm });
    },

    setLoopBeats(beats: 16 | 32) {
      state.loopBeats = beats;
      loop.setBeats(beats);
      const r = loop.region;
      if (r) {
        state.loopRegion = r;
        state.inferredBpm = loop.inferredBpm;
        state.targetBpm = state.inferredBpm;
        state.pitchOffset = 0;
        loop.targetBpm = state.targetBpm;
        if (currentFilename)
          saveRegion(currentFilename, { ...r, beats, detectedBpm: state.inferredBpm });
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
        const startTime = audioCtx.currentTime + START_LATENCY;
        loop.startAt(startTime);
        state.loopPlaying = true;
      }
    },

    cueStart() {
      if (!state.trackLoaded) return;
      if (state.cueing || state.loopPlaying) return;
      state.cueing = true;
      const startTime = audioCtx.currentTime + START_LATENCY;
      loop.startAt(startTime);
      state.loopPlaying = true;
    },

    cueEnd() {
      if (!state.cueing) return;
      state.cueing = false;
      loop.stop();
      state.loopPlaying = false;
      loop.cue();
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
