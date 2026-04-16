import { defineStore } from 'pinia';
import { reactive } from 'vue';
import { invoke } from '@tauri-apps/api/core';

export type DeckId = 'A' | 'B';
export type DeckMode = 'edit' | 'play';

export type LoopRegion = {
  startSec: number;
  endSec: number;
  beats: number;
};

// Shape of data returned by the Rust load_track command
export type TrackData = {
  duration: number;
  sampleRate: number;
  bpm: number | null;
  silenceEnd: number;
};

export const PITCH_RANGE = 10;

const NUDGE_PERCENT = 4;
export const EQ_MIN_DB = -26;
export const EQ_MAX_DB = 6;

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
  const raw = loadSavedTracks()[filename] as
    | (SavedTrack & { startSec?: number; endSec?: number; beats?: number; detectedBpm?: number })
    | undefined;
  if (!raw) return null;
  if (raw.trackBpm && raw.beatOffset !== undefined && !raw.startSec) return raw as SavedTrack;
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
  let positionCache = 0;
  let rafId: number | null = null;
  let pollInFlight = false;
  let currentFilename = '';

  function startPolling() {
    if (rafId !== null) return;
    function tick() {
      if (!state.loopPlaying) {
        rafId = null;
        return;
      }
      if (!pollInFlight) {
        pollInFlight = true;
        invoke<number>('get_position', { deck: id })
          .then((pos) => {
            positionCache = pos;
            pollInFlight = false;
          })
          .catch(() => {
            pollInFlight = false;
          });
      }
      rafId = requestAnimationFrame(tick);
    }
    rafId = requestAnimationFrame(tick);
  }

  function stopPolling() {
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
    pollInFlight = false;
  }

  const state = reactive({
    id,
    accent,
    mode: 'play' as DeckMode,

    trackName: '',
    trackLoaded: false,
    trackData: null as TrackData | null,
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
      return state.loopPlaying ? positionCache : null;
    },

    get phase(): number {
      if (state.trackBpm === null || !state.loopPlaying) return 0;
      const pos = positionCache;
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
      if (state.trackBpm !== null) {
        invoke('set_playback_rate', { deck: id, rate: clamped / state.trackBpm });
      }
    },

    setPitchOffset(pct: number) {
      if (state.trackBpm === null) return;
      state.pitchOffset = Math.max(-PITCH_RANGE, Math.min(PITCH_RANGE, pct));
      state.targetBpm = state.trackBpm * (1 + state.pitchOffset / 100);
      invoke('set_playback_rate', {
        deck: id,
        rate: state.targetBpm / state.trackBpm
      });
    },

    async loadTrack(path: string, onNeedBpmInput: () => void) {
      if (state.loopPlaying) {
        await invoke('stop', { deck: id });
        state.loopPlaying = false;
        stopPolling();
      }
      state.cueing = false;
      state.nudging = null;
      state.loopRegion = null;
      state.loopActive = false;
      state.trackData = null;
      positionCache = 0;

      currentFilename = path.split('/').pop() ?? path;
      const saved = getSavedTrack(currentFilename);
      const analyze = !saved;

      state.detecting = analyze;

      let info: TrackData;
      try {
        info = await invoke<TrackData>('load_track', { deck: id, path, analyze });
      } catch (err) {
        state.detecting = false;
        throw err;
      }

      state.trackName = currentFilename;
      state.trackData = info;
      state.trackLoaded = true;
      state.detecting = false;

      if (saved) {
        console.log(
          `[deck ${id}] restored saved track: bpm=${saved.trackBpm} cuePoint=${saved.cuePoint ?? saved.beatOffset}`
        );
        state.trackBpm = saved.trackBpm;
        state.beatOffset = saved.beatOffset;
        state.cuePoint = saved.cuePoint ?? saved.beatOffset;
        state.targetBpm = saved.trackBpm;
        state.pitchOffset = 0;
        positionCache = state.cuePoint;
        await invoke('set_playback_rate', { deck: id, rate: 1.0 });
        await invoke('seek', { deck: id, sec: state.cuePoint });
        if (saved.loopRegion) {
          state.loopRegion = saved.loopRegion;
          await invoke('set_loop_region', {
            deck: id,
            startSec: saved.loopRegion.startSec,
            endSec: saved.loopRegion.endSec
          });
        }
        state.mode = 'edit';
        return;
      }

      console.log(`[deck ${id}] load_track analysis result:`, {
        duration: info.duration,
        bpm: info.bpm,
        silenceEnd: info.silenceEnd
      });

      const detectedBpm = info.bpm ?? 0;
      const silenceEnd = info.silenceEnd;

      if (detectedBpm > 0) {
        console.log(
          `[deck ${id}] using detected bpm=${detectedBpm} silenceEnd=${silenceEnd}s as beat offset`
        );
        state.setTrackBpm(detectedBpm, silenceEnd);
        state.mode = 'edit';
      } else {
        console.log(`[deck ${id}] no BPM detected, requesting manual input`);
        onNeedBpmInput();
      }
    },

    setEditMode() {
      state.mode = 'edit';
    },

    setPlayMode() {
      state.mode = 'play';
    },

    setTrackBpm(bpm: number, beatOffsetSec = state.beatOffset) {
      state.trackBpm = bpm;
      state.beatOffset = beatOffsetSec;
      state.cuePoint = beatOffsetSec;
      state.targetBpm = bpm;
      state.pitchOffset = 0;
      positionCache = beatOffsetSec;
      invoke('set_playback_rate', { deck: id, rate: 1.0 });
      invoke('seek', { deck: id, sec: beatOffsetSec });
      if (currentFilename) {
        saveTrack(currentFilename, {
          trackBpm: bpm,
          beatOffset: beatOffsetSec,
          cuePoint: beatOffsetSec,
          loopRegion: state.loopRegion
            ? {
                startSec: state.loopRegion.startSec,
                endSec: state.loopRegion.endSec,
                beats: state.loopRegion.beats
              }
            : undefined
        });
      }
    },

    setBeatOffset(sec: number) {
      state.beatOffset = sec;
      if (currentFilename && state.trackBpm !== null) {
        saveTrack(currentFilename, {
          trackBpm: state.trackBpm,
          beatOffset: sec,
          cuePoint: state.cuePoint,
          loopRegion: state.loopRegion
            ? {
                startSec: state.loopRegion.startSec,
                endSec: state.loopRegion.endSec,
                beats: state.loopRegion.beats
              }
            : undefined
        });
      }
    },

    setLoopRegion(region: LoopRegion) {
      state.loopRegion = region;
      invoke('set_loop_region', {
        deck: id,
        startSec: region.startSec,
        endSec: region.endSec
      });
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
      invoke('set_loop_region', { deck: id, startSec, endSec: startSec + dur });
      if (currentFilename && state.trackBpm !== null) {
        saveTrack(currentFilename, {
          trackBpm: state.trackBpm,
          beatOffset: state.beatOffset,
          cuePoint: state.cuePoint,
          loopRegion: { startSec: region.startSec, endSec: region.endSec, beats: region.beats }
        });
      }
    },

    setLoopIn() {
      if (!state.trackLoaded || state.trackBpm === null) return;
      const inSec = quantizeToBeat(positionCache, state.trackBpm, state.beatOffset);
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
      const outSec = quantizeToBeat(positionCache, state.trackBpm, state.beatOffset);
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
        await invoke('stop', { deck: id });
        state.loopPlaying = false;
        stopPolling();
      } else {
        await invoke('play', { deck: id });
        state.loopPlaying = true;
        startPolling();
      }
    },

    async cueStart() {
      if (!state.trackLoaded || state.cueing || state.loopPlaying) return;
      // playhead is not at the cue point: move cue to playhead, do not start playback
      if (Math.abs(positionCache - state.cuePoint) > 0.001) {
        state.cuePoint = positionCache;
        if (currentFilename && state.trackBpm !== null) {
          saveTrack(currentFilename, {
            trackBpm: state.trackBpm,
            beatOffset: state.beatOffset,
            cuePoint: positionCache,
            loopRegion: state.loopRegion
              ? {
                  startSec: state.loopRegion.startSec,
                  endSec: state.loopRegion.endSec,
                  beats: state.loopRegion.beats
                }
              : undefined
          });
        }
        return;
      }
      // playhead is at the cue point: momentary playback while button is held
      state.cueing = true;
      state.loopPlaying = true;
      await invoke('play', { deck: id, fromSec: state.cuePoint });
      startPolling();
    },

    async cueEnd() {
      if (!state.cueing) return;
      state.cueing = false;
      state.loopPlaying = false;
      stopPolling();
      await invoke('stop', { deck: id });
      positionCache = state.cuePoint;
      await invoke('seek', { deck: id, sec: state.cuePoint });
    },

    async setCueAndStop() {
      if (!state.trackLoaded || !state.loopPlaying || state.cueing) return;
      const pos = positionCache;
      state.cuePoint = pos;
      state.loopPlaying = false;
      stopPolling();
      await invoke('stop', { deck: id });
      positionCache = pos;
      await invoke('seek', { deck: id, sec: pos });
      if (currentFilename && state.trackBpm !== null) {
        saveTrack(currentFilename, {
          trackBpm: state.trackBpm,
          beatOffset: state.beatOffset,
          cuePoint: pos,
          loopRegion: state.loopRegion
            ? {
                startSec: state.loopRegion.startSec,
                endSec: state.loopRegion.endSec,
                beats: state.loopRegion.beats
              }
            : undefined
        });
      }
    },

    async stopAtCue() {
      if (!state.loopPlaying || state.cueing) return;
      await invoke('stop', { deck: id });
      state.loopPlaying = false;
      stopPolling();
      positionCache = state.cuePoint;
      await invoke('seek', { deck: id, sec: state.cuePoint });
    },

    seekTo(sec: number) {
      const clamped = Math.max(0, sec);
      positionCache = clamped;
      invoke('seek', { deck: id, sec: clamped });
    },

    getPlayheadPosition(): number {
      return positionCache;
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
      invoke('set_nudge', { deck: id, percent: offset });
    },

    nudgeEnd() {
      state.nudging = null;
      invoke('set_nudge', { deck: id, percent: 0 });
    },

    get playing(): boolean {
      return state.loopPlaying && !state.cueing;
    },

    getWaveformRegion(startSec: number, endSec: number, numPoints: number): Promise<number[]> {
      return invoke<number[]>('get_waveform_region', { deck: id, startSec, endSec, numPoints });
    },

    destroy() {
      stopPolling();
      invoke('stop', { deck: id }).catch(() => {});
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

  return {
    deckA, deckB, decks,
    destroy,
  };
});
