import { computed, type Ref } from 'vue';
import type { SessionEvent, ParsedSession } from '@renderer/stores/session';
import { DEFAULT_MASTER_GAIN } from '@renderer/stores/mixer';
import { PITCH_RANGE_OPTIONS } from '@renderer/stores/settings';

export type Clip = {
  deck: string;
  sessionStartMs: number;
  sessionEndMs: number;
  trackPath: string;
  trackName: string;
  trackStartSec: number;
  playbackRate: number;
  // Clips emitted together form one editable unit: loop iterations share a
  // blockId; a regular play segment is a block of its own.
  blockId: number;
  loop: { startSec: number; endSec: number } | null;
};

export type LoadedSpan = {
  deck: string;
  trackPath: string;
  trackName: string;
  startMs: number;
  endMs: number;
};

export type LanePoint = { ms: number; value: number };

export type FilterActiveSpan = { startMs: number; endMs: number };

export type NudgeSpan = { startMs: number; endMs: number; percent: number };

export type DeckLanes = {
  gain: LanePoint[];
  eqLow: LanePoint[];
  eqMid: LanePoint[];
  eqHigh: LanePoint[];
  filter: LanePoint[];
  rate: LanePoint[];
  rateMin: number;
  rateMax: number;
  filterActive: FilterActiveSpan[];
};

export type MasterLanes = {
  gain: LanePoint[];
};

export const DEFAULT_GAIN = 1;
export const DEFAULT_EQ_DB = 0;
export const DEFAULT_FILTER_VALUE = 0;
export const DEFAULT_RATE = 1;

// The lane range is derived from the session's own rate values (not the live
// pitch-range setting) because a session may have been recorded under a
// different setting. Ranges below 8% are skipped so a flat session still gets
// a usable drawing range.
const MIN_RATE_LANE_RANGE_PCT = 8;
const RATE_RANGE_STEPS_PCT = PITCH_RANGE_OPTIONS.filter((pct) => pct >= MIN_RATE_LANE_RANGE_PCT);

export function rateRangePctFor(maxDeviationPct: number): number {
  return (
    RATE_RANGE_STEPS_PCT.find((pct) => pct >= maxDeviationPct) ??
    RATE_RANGE_STEPS_PCT[RATE_RANGE_STEPS_PCT.length - 1]
  );
}

type DeckState = {
  path: string | null;
  trackPosSec: number;
  rate: number;
  loopStartSec: number | null;
  loopEndSec: number | null;
  loopActive: boolean;
  loopEngagedMs: number | null;
  clipStartMs: number | null;
  clipTrackStartSec: number;
  clipRate: number;
  clipPath: string | null;
  loadSpanStartMs: number | null;
  loadSpanPath: string | null;
};

function makeDeckState(): DeckState {
  return {
    path: null,
    trackPosSec: 0,
    rate: 1,
    loopStartSec: null,
    loopEndSec: null,
    loopActive: false,
    loopEngagedMs: null,
    clipStartMs: null,
    clipTrackStartSec: 0,
    clipRate: 1,
    clipPath: null,
    loadSpanStartMs: null,
    loadSpanPath: null
  };
}

function startClip(deck: DeckState, ms: number) {
  deck.clipStartMs = ms;
  deck.clipTrackStartSec = deck.trackPosSec;
  deck.clipRate = deck.rate;
  deck.clipPath = deck.path;
}

function engageLoop(deck: DeckState, ms: number) {
  deck.loopActive = true;
  deck.loopEngagedMs = ms;
  deck.clipPath = deck.path;
  deck.clipRate = deck.rate;
  deck.clipStartMs = ms;
  if (deck.loopStartSec !== null) deck.trackPosSec = deck.loopStartSec;
}

function loopExitTrackPos(deck: DeckState, endMs: number): number {
  if (
    deck.loopEngagedMs === null ||
    deck.loopStartSec === null ||
    deck.loopEndSec === null ||
    deck.clipRate <= 0
  ) {
    return deck.trackPosSec;
  }
  const loopDurMs = ((deck.loopEndSec - deck.loopStartSec) / deck.clipRate) * 1000;
  if (loopDurMs <= 0) return deck.trackPosSec;
  const elapsedMs = endMs - deck.loopEngagedMs;
  const partialMs = elapsedMs % loopDurMs;
  // nudge_factor is not tracked here; loop duration may be slightly off during an active nudge,
  // but nudge is transient so the error is negligible over full iterations
  return deck.loopStartSec + (partialMs / 1000) * deck.clipRate;
}

function finalizeClip(
  deck: DeckState,
  deckId: string,
  endMs: number,
  out: Clip[],
  nameForPath: (path: string) => string,
  allocateBlockId: () => number
) {
  if (
    deck.loopActive &&
    deck.loopEngagedMs !== null &&
    deck.loopStartSec !== null &&
    deck.loopEndSec !== null &&
    deck.clipPath !== null
  ) {
    const loopDurSec = deck.loopEndSec - deck.loopStartSec;
    if (loopDurSec > 0 && deck.clipRate > 0) {
      const loopDurMs = (loopDurSec / deck.clipRate) * 1000;
      const loopPath = deck.clipPath;
      const loopStartSec = deck.loopStartSec;
      const loopEndSec = deck.loopEndSec;
      const loopRate = deck.clipRate;
      const blockId = allocateBlockId();
      let iterStart = deck.loopEngagedMs;
      while (iterStart < endMs) {
        const iterEnd = Math.min(iterStart + loopDurMs, endMs);
        out.push({
          deck: deckId,
          sessionStartMs: iterStart,
          sessionEndMs: iterEnd,
          trackPath: loopPath,
          trackName: nameForPath(loopPath),
          trackStartSec: loopStartSec,
          playbackRate: loopRate,
          blockId,
          loop: { startSec: loopStartSec, endSec: loopEndSec }
        });
        iterStart += loopDurMs;
      }
    }
    deck.loopActive = false;
    deck.loopEngagedMs = null;
    deck.clipStartMs = null;
  } else if (deck.clipStartMs !== null) {
    // A zero-length clip emits nothing, but the deck still stopped: leaving
    // clipStartMs set would swallow the next play (the engine's stop always
    // stops, regardless of how long the clip was).
    if (deck.clipPath !== null && endMs > deck.clipStartMs) {
      out.push({
        deck: deckId,
        sessionStartMs: deck.clipStartMs,
        sessionEndMs: endMs,
        trackPath: deck.clipPath,
        trackName: nameForPath(deck.clipPath),
        trackStartSec: deck.clipTrackStartSec,
        playbackRate: deck.clipRate,
        blockId: allocateBlockId(),
        loop: null
      });
    }
    deck.clipStartMs = null;
  }
}

function finalizeLoadedSpan(
  deck: DeckState,
  deckId: string,
  endMs: number,
  out: LoadedSpan[],
  nameForPath: (path: string) => string
) {
  if (deck.loadSpanStartMs === null || deck.loadSpanPath === null) return;
  out.push({
    deck: deckId,
    trackPath: deck.loadSpanPath,
    trackName: nameForPath(deck.loadSpanPath),
    startMs: deck.loadSpanStartMs,
    endMs
  });
  deck.loadSpanStartMs = null;
  deck.loadSpanPath = null;
}

// Shared sequence for all loop-exit events: compute where in the loop we landed,
// finalize the loop iterations as clips, then start a new regular clip.
function exitLoopAndContinue(
  deck: DeckState,
  deckId: string,
  ms: number,
  clips: Clip[],
  nameForPath: (path: string) => string,
  allocateBlockId: () => number
) {
  deck.trackPosSec = loopExitTrackPos(deck, ms);
  finalizeClip(deck, deckId, ms, clips, nameForPath, allocateBlockId);
  startClip(deck, ms);
}

export function buildClips(
  events: SessionEvent[],
  nameForPath: (path: string) => string
): { clips: Clip[]; loadedSpans: LoadedSpan[] } {
  const deckStates: Record<string, DeckState> = {};
  const clips: Clip[] = [];
  let nextBlockId = 0;
  const allocateBlockId = () => nextBlockId++;
  const loadedSpans: LoadedSpan[] = [];
  const getOrCreate = (id: string) => (deckStates[id] ??= makeDeckState());

  for (const ev of events) {
    const deckId = ev.deck;
    if (!deckId) continue;
    const deck = getOrCreate(deckId);

    switch (ev.type) {
      case 'deck_snapshot':
        deck.path = ev.path ?? null;
        deck.rate = ev.playback_rate ?? 1;
        deck.trackPosSec = ev.position_sec ?? 0;
        // loop start = cue_point by invariant; deck_snapshot logs cue_point_sec, not loop_start_sec
        deck.loopStartSec = ev.loop_active ? (ev.cue_point_sec ?? null) : null;
        deck.loopEndSec = ev.loop_active ? (ev.loop_end_sec ?? null) : null;
        if (deck.path !== null && deck.loadSpanStartMs === null) {
          deck.loadSpanStartMs = 0;
          deck.loadSpanPath = deck.path;
        }
        if (ev.is_playing) {
          if (ev.loop_active && deck.loopStartSec !== null && deck.loopEndSec !== null) {
            engageLoop(deck, ev.elapsed_ms);
          } else {
            startClip(deck, ev.elapsed_ms);
          }
        }
        break;

      case 'load_track':
        finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath, allocateBlockId);
        finalizeLoadedSpan(deck, deckId, ev.elapsed_ms, loadedSpans, nameForPath);
        deck.path = ev.path ?? null;
        deck.loadSpanStartMs = ev.elapsed_ms;
        deck.loadSpanPath = deck.path;
        deck.trackPosSec = 0;
        deck.loopStartSec = null;
        deck.loopEndSec = null;
        deck.loopActive = false;
        deck.loopEngagedMs = null;
        break;

      case 'eject_track':
        finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath, allocateBlockId);
        finalizeLoadedSpan(deck, deckId, ev.elapsed_ms, loadedSpans, nameForPath);
        deck.path = null;
        deck.trackPosSec = 0;
        deck.loopActive = false;
        deck.loopEngagedMs = null;
        break;

      case 'play':
        // play may carry an explicit position (Rust play(deck, fromSec));
        // recorded plays from toggle_play never do, but edited sessions
        // (clip move/trim) synthesize them.
        if (ev.sec !== undefined) deck.trackPosSec = ev.sec;
        if (deck.clipStartMs === null && !deck.loopActive) startClip(deck, ev.elapsed_ms);
        break;

      case 'cue_preview_start':
        // Rust jumps the deck to the cue point; mirror it so the clip's
        // trackStartSec doesn't depend on earlier position side effects.
        if (ev.cue_point_sec !== undefined) deck.trackPosSec = ev.cue_point_sec;
        if (deck.clipStartMs === null && !deck.loopActive) startClip(deck, ev.elapsed_ms);
        break;

      case 'stop':
      case 'stopped_at_cue':
      case 'stop_at_cue':
      case 'cue_set_and_stop':
        // cue_set_and_stop: user pressed CUE while playing, stops and moves cue to current position
        finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath, allocateBlockId);
        if (ev.cue_point_sec !== undefined) deck.trackPosSec = ev.cue_point_sec;
        break;

      case 'cue_preview_end':
        finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath, allocateBlockId);
        if (ev.cue_point_sec !== undefined) deck.trackPosSec = ev.cue_point_sec;
        break;

      // cue_move fires when the user presses CUE while stopped and away from the cue point;
      // the deck is not playing, so no clip to finalize. Rust also clears any active loop region.
      case 'cue_move':
        deck.loopStartSec = null;
        deck.loopEndSec = null;
        deck.loopActive = false;
        deck.loopEngagedMs = null;
        if (ev.cue_point_sec !== undefined) deck.trackPosSec = ev.cue_point_sec;
        break;

      case 'seek':
        if (ev.sec !== undefined) {
          if (deck.clipStartMs !== null && !deck.loopActive) {
            finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath, allocateBlockId);
            deck.trackPosSec = ev.sec;
            startClip(deck, ev.elapsed_ms);
          } else {
            deck.trackPosSec = ev.sec;
          }
        }
        break;

      case 'set_playback_rate':
        if (ev.rate !== undefined) deck.rate = ev.rate;
        break;

      case 'loop_out':
        // start_sec is the loop start (= cue_point in Rust at the moment loop_out fires)
        deck.loopStartSec = ev.start_sec ?? null;
        deck.loopEndSec = ev.end_sec ?? null;
        if (deck.loopStartSec !== null && deck.loopEndSec !== null) {
          finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath, allocateBlockId);
          engageLoop(deck, ev.elapsed_ms);
        }
        break;

      case 'loop_in':
        // loop_in always clears the loop region in Rust; if we were looping, exit first
        if (deck.loopActive) {
          exitLoopAndContinue(deck, deckId, ev.elapsed_ms, clips, nameForPath, allocateBlockId);
        }
        deck.loopStartSec = null;
        deck.loopEndSec = null;
        break;

      case 'exit_loop':
        // set_loop_active(false) also logs exit_loop, so both paths reach here
        if (deck.loopActive) {
          exitLoopAndContinue(deck, deckId, ev.elapsed_ms, clips, nameForPath, allocateBlockId);
        }
        break;

      case 'reloop':
        if (!deck.loopActive && deck.loopStartSec !== null && deck.loopEndSec !== null) {
          finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath, allocateBlockId);
          engageLoop(deck, ev.elapsed_ms);
        }
        break;
    }
  }

  const lastMs = events[events.length - 1]?.elapsed_ms ?? 0;
  for (const [deckId, deck] of Object.entries(deckStates)) {
    finalizeClip(deck, deckId, lastMs, clips, nameForPath, allocateBlockId);
    finalizeLoadedSpan(deck, deckId, lastMs, loadedSpans, nameForPath);
  }

  return { clips, loadedSpans };
}

function makeDeckLanes(): DeckLanes {
  return {
    gain: [{ ms: 0, value: DEFAULT_GAIN }],
    eqLow: [{ ms: 0, value: DEFAULT_EQ_DB }],
    eqMid: [{ ms: 0, value: DEFAULT_EQ_DB }],
    eqHigh: [{ ms: 0, value: DEFAULT_EQ_DB }],
    filter: [{ ms: 0, value: DEFAULT_FILTER_VALUE }],
    rate: [{ ms: 0, value: DEFAULT_RATE }],
    rateMin: 1 - RATE_RANGE_STEPS_PCT[0] / 100,
    rateMax: 1 + RATE_RANGE_STEPS_PCT[0] / 100,
    filterActive: []
  };
}

function extendToEnd(points: LanePoint[], durationMs: number) {
  const last = points[points.length - 1];
  if (last && last.ms < durationMs) points.push({ ms: durationMs, value: last.value });
}

export function buildLanes(
  events: SessionEvent[],
  durationMs: number
): {
  deckLanes: Record<string, DeckLanes>;
  masterLanes: MasterLanes;
  deckNudges: Record<string, NudgeSpan[]>;
} {
  const deckLanes: Record<string, DeckLanes> = {};
  const filterActiveSinceMs: Record<string, number | null> = {};
  const nudgeSince: Record<string, { startMs: number; percent: number } | null> = {};
  const deckNudges: Record<string, NudgeSpan[]> = {};
  const masterLanes: MasterLanes = { gain: [{ ms: 0, value: DEFAULT_MASTER_GAIN }] };

  const getOrCreate = (id: string) => {
    if (!deckLanes[id]) {
      deckLanes[id] = makeDeckLanes();
      filterActiveSinceMs[id] = null;
      nudgeSince[id] = null;
      deckNudges[id] = [];
    }
    return deckLanes[id];
  };

  for (const ev of events) {
    const deckId = ev.deck;
    switch (ev.type) {
      case 'set_volume':
        if (deckId && ev.gain !== undefined) {
          getOrCreate(deckId).gain.push({ ms: ev.elapsed_ms, value: ev.gain });
        }
        break;

      case 'set_eq':
        if (deckId && ev.band !== undefined && ev.db !== undefined) {
          const auto = getOrCreate(deckId);
          const lane =
            ev.band === 'low' ? auto.eqLow : ev.band === 'mid' ? auto.eqMid : auto.eqHigh;
          lane.push({ ms: ev.elapsed_ms, value: ev.db });
        }
        break;

      case 'set_filter':
        if (deckId && ev.value !== undefined) {
          getOrCreate(deckId).filter.push({ ms: ev.elapsed_ms, value: ev.value });
        }
        break;

      case 'deck_snapshot':
        if (deckId && ev.playback_rate !== undefined) {
          getOrCreate(deckId).rate.push({ ms: ev.elapsed_ms, value: ev.playback_rate });
        }
        break;

      case 'set_playback_rate':
        if (deckId && ev.rate !== undefined) {
          getOrCreate(deckId).rate.push({ ms: ev.elapsed_ms, value: ev.rate });
        }
        break;

      case 'set_filter_active':
        if (deckId && ev.active !== undefined) {
          getOrCreate(deckId);
          if (ev.active && filterActiveSinceMs[deckId] === null) {
            filterActiveSinceMs[deckId] = ev.elapsed_ms;
          } else if (!ev.active && filterActiveSinceMs[deckId] !== null) {
            deckLanes[deckId].filterActive.push({
              startMs: filterActiveSinceMs[deckId]!,
              endMs: ev.elapsed_ms
            });
            filterActiveSinceMs[deckId] = null;
          }
        }
        break;

      // A nudge interval runs from the first non-zero `percent` event to the
      // following `percent: 0` event for that deck (mirrors filterActive pairing).
      case 'set_nudge':
        if (deckId && ev.percent !== undefined) {
          getOrCreate(deckId);
          if (ev.percent !== 0) {
            if (nudgeSince[deckId] === null) {
              nudgeSince[deckId] = { startMs: ev.elapsed_ms, percent: ev.percent };
            } else {
              nudgeSince[deckId]!.percent = ev.percent;
            }
          } else if (nudgeSince[deckId] !== null) {
            const span = nudgeSince[deckId]!;
            deckNudges[deckId].push({
              startMs: span.startMs,
              endMs: ev.elapsed_ms,
              percent: span.percent
            });
            nudgeSince[deckId] = null;
          }
        }
        break;

      case 'set_master_gain':
        if (ev.gain !== undefined) {
          masterLanes.gain.push({ ms: ev.elapsed_ms, value: ev.gain });
        }
        break;
    }
  }

  for (const [deckId, auto] of Object.entries(deckLanes)) {
    const sinceMs = filterActiveSinceMs[deckId];
    if (sinceMs !== null && sinceMs !== undefined) {
      auto.filterActive.push({ startMs: sinceMs, endMs: durationMs });
    }
    const nudge = nudgeSince[deckId];
    if (nudge) {
      deckNudges[deckId].push({
        startMs: nudge.startMs,
        endMs: durationMs,
        percent: nudge.percent
      });
    }
    extendToEnd(auto.gain, durationMs);
    extendToEnd(auto.eqLow, durationMs);
    extendToEnd(auto.eqMid, durationMs);
    extendToEnd(auto.eqHigh, durationMs);
    extendToEnd(auto.filter, durationMs);
    extendToEnd(auto.rate, durationMs);
  }
  extendToEnd(masterLanes.gain, durationMs);

  let maxRateDeviationPct = 0;
  for (const auto of Object.values(deckLanes)) {
    for (const p of auto.rate) {
      maxRateDeviationPct = Math.max(maxRateDeviationPct, Math.abs(p.value - 1) * 100);
    }
  }
  const rangePct = rateRangePctFor(maxRateDeviationPct);
  for (const auto of Object.values(deckLanes)) {
    auto.rateMin = 1 - rangePct / 100;
    auto.rateMax = 1 + rangePct / 100;
  }

  return { deckLanes, masterLanes, deckNudges };
}

export type { ParsedSession };

function defaultNameForPath(path: string): string {
  return (
    path
      .split('/')
      .pop()
      ?.replace(/\.[^.]+$/, '') ?? path
  );
}

export function useSessionTimeline(
  session: Ref<ParsedSession | null>,
  nameForPath: (path: string) => string = defaultNameForPath
) {
  const built = computed(() => {
    if (!session.value) return { clips: [] as Clip[], loadedSpans: [] as LoadedSpan[] };
    return buildClips(session.value.events, nameForPath);
  });

  const lanesBuilt = computed<{
    deckLanes: Record<string, DeckLanes>;
    masterLanes: MasterLanes;
    deckNudges: Record<string, NudgeSpan[]>;
  }>(() => {
    if (!session.value) {
      return { deckLanes: {}, masterLanes: { gain: [] }, deckNudges: {} };
    }
    return buildLanes(session.value.events, session.value.durationMs);
  });

  const clips = computed(() => built.value.clips);
  const loadedSpans = computed(() => built.value.loadedSpans);
  const deckLanes = computed(() => lanesBuilt.value.deckLanes);
  const masterLanes = computed(() => lanesBuilt.value.masterLanes);
  const deckNudges = computed(() => lanesBuilt.value.deckNudges);

  return { clips, loadedSpans, deckLanes, masterLanes, deckNudges };
}
