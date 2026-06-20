import type { SessionEvent } from '@renderer/stores/session';
import type { EditableLaneKey, LanePoint } from '@renderer/utils/types';
import {
  DEFAULT_GAIN,
  DEFAULT_EQ_DB,
  DEFAULT_FILTER_VALUE,
  DEFAULT_RATE
} from '@renderer/composables/useSessionTimeline';
import {
  EQ_MIN_DB,
  EQ_MAX_DB,
  FILTER_DEAD_ZONE,
  DEFAULT_MASTER_GAIN
} from '@renderer/stores/mixer';
import {
  spliceLaneEvents as coreSpliceLaneEvents,
  deleteNudgeRange as coreDeleteNudgeRange,
  relocateEventPaths as coreRelocateEventPaths
} from '@renderer/utils/sessionCore';

export type LaneSpec = {
  key: EditableLaneKey;
  min: number;
  max: number;
  defaultValue: number;
  epsilon: number;
  snap?: (value: number) => number;
  matches: (event: SessionEvent, deck: string) => boolean;
  valueAt: (event: SessionEvent, deck: string) => number | undefined;
  makeEvent: (ms: number, value: number, deck: string) => SessionEvent;
};

function eqSpec(band: 'low' | 'mid' | 'high'): LaneSpec {
  return {
    key: band === 'low' ? 'eqLow' : band === 'mid' ? 'eqMid' : 'eqHigh',
    min: EQ_MIN_DB,
    max: EQ_MAX_DB,
    defaultValue: DEFAULT_EQ_DB,
    epsilon: 0.25,
    matches: (event, deck) => event.type === 'set_eq' && event.deck === deck && event.band === band,
    valueAt: (event, deck) =>
      event.type === 'set_eq' && event.deck === deck && event.band === band ? event.db : undefined,
    makeEvent: (ms, value, deck) => ({ elapsed_ms: ms, type: 'set_eq', deck, band, db: value })
  };
}

export function laneSpecFor(
  key: EditableLaneKey,
  opts: { rateMin?: number; rateMax?: number } = {}
): LaneSpec {
  switch (key) {
    case 'gain':
      return {
        key,
        min: 0,
        max: 1,
        defaultValue: DEFAULT_GAIN,
        epsilon: 0.01,
        matches: (event, deck) => event.type === 'set_volume' && event.deck === deck,
        valueAt: (event, deck) =>
          event.type === 'set_volume' && event.deck === deck ? event.gain : undefined,
        makeEvent: (ms, value, deck) => ({ elapsed_ms: ms, type: 'set_volume', deck, gain: value })
      };
    case 'eqLow':
      return eqSpec('low');
    case 'eqMid':
      return eqSpec('mid');
    case 'eqHigh':
      return eqSpec('high');
    case 'filter':
      return {
        key,
        min: -1,
        max: 1,
        defaultValue: DEFAULT_FILTER_VALUE,
        epsilon: 0.01,
        snap: (value) => (Math.abs(value) <= FILTER_DEAD_ZONE ? 0 : value),
        matches: (event, deck) => event.type === 'set_filter' && event.deck === deck,
        valueAt: (event, deck) =>
          event.type === 'set_filter' && event.deck === deck ? event.value : undefined,
        makeEvent: (ms, value, deck) => ({ elapsed_ms: ms, type: 'set_filter', deck, value })
      };
    case 'rate':
      return {
        key,
        min: opts.rateMin ?? 0.92,
        max: opts.rateMax ?? 1.08,
        defaultValue: DEFAULT_RATE,
        epsilon: 0.0005,
        matches: (event, deck) => event.type === 'set_playback_rate' && event.deck === deck,
        valueAt: (event, deck) => {
          if (event.type === 'set_playback_rate' && event.deck === deck) return event.rate;
          if (event.type === 'deck_snapshot' && event.deck === deck) return event.playback_rate;
          return undefined;
        },
        makeEvent: (ms, value, deck) => ({
          elapsed_ms: ms,
          type: 'set_playback_rate',
          deck,
          rate: value
        })
      };
    case 'masterGain':
      return {
        key,
        min: 0,
        max: 1,
        defaultValue: DEFAULT_MASTER_GAIN,
        epsilon: 0.01,
        matches: (event) => event.type === 'set_master_gain',
        valueAt: (event) => (event.type === 'set_master_gain' ? event.gain : undefined),
        makeEvent: (ms, value) => ({ elapsed_ms: ms, type: 'set_master_gain', gain: value })
      };
  }
}

// Replaces this lane's events inside [t0, t1] with the drawn points and restores
// the original value at t1, so everything after the gesture sounds unchanged.
// Returns a new array; the input and its event objects are never mutated.
export function spliceLaneEvents(
  events: SessionEvent[],
  spec: LaneSpec,
  deck: string,
  t0: number,
  t1: number,
  points: LanePoint[]
): SessionEvent[] {
  return coreSpliceLaneEvents(events, spec.key, deck, t0, t1, points, spec.min, spec.max);
}

// Removes a nudge span: every set_nudge for the deck in [t0, t1], including
// the closing zero. A non-zero event exactly at t1 is the opener of an
// adjacent span and is kept. Spans always start from and return to 0, so no
// restore event is needed. Returns the input array unchanged when nothing
// matches, so callers relying on reference equality can skip a no-op edit.
export function deleteNudgeRange(
  events: SessionEvent[],
  deck: string,
  t0: number,
  t1: number
): SessionEvent[] {
  const inRange = (event: SessionEvent) =>
    event.type === 'set_nudge' &&
    event.deck === deck &&
    event.elapsed_ms >= t0 &&
    event.elapsed_ms <= t1 &&
    !(event.elapsed_ms === t1 && event.percent !== 0);
  if (!events.some(inRange)) return events;
  return coreDeleteNudgeRange(events, deck, t0, t1);
}

// Rewrites event track paths after the user relocates missing files. Returns
// the input array unchanged when no event carries a mapped path, so callers
// relying on reference equality (dirty check, undo) can skip a no-op edit.
export function relocateEventPaths(
  events: SessionEvent[],
  mapping: Record<string, string>
): SessionEvent[] {
  if (!events.some((event) => event.path !== undefined && mapping[event.path] !== undefined)) {
    return events;
  }
  return coreRelocateEventPaths(events, mapping);
}

export function formatLaneValue(key: EditableLaneKey, value: number): string {
  switch (key) {
    case 'gain':
    case 'masterGain':
      return value.toFixed(2);
    case 'eqLow':
    case 'eqMid':
    case 'eqHigh':
      return `${value > 0 ? '+' : ''}${value.toFixed(1)} dB`;
    case 'filter':
      return value.toFixed(2);
    case 'rate': {
      const pct = (value - 1) * 100;
      return `${pct > 0 ? '+' : ''}${pct.toFixed(1)}%`;
    }
  }
}
