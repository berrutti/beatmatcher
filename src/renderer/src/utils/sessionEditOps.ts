import type { SessionEvent } from '@renderer/stores/session';
import {
  type LanePoint,
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

export type EditableLaneKey =
  | 'gain'
  | 'eqLow'
  | 'eqMid'
  | 'eqHigh'
  | 'filter'
  | 'rate'
  | 'masterGain';

export type LaneSpec = {
  min: number;
  max: number;
  defaultValue: number;
  epsilon: number;
  snap?: (value: number) => number;
  matches: (event: SessionEvent, deck: string) => boolean;
  valueAt: (event: SessionEvent, deck: string) => number | undefined;
  makeEvent: (ms: number, value: number, deck: string) => SessionEvent;
};

// Gestures spanning less time than this are rejected: the value would change
// and restore almost instantly, which is inaudible and renders as a bare
// vertical line on the lane.
export const MIN_GESTURE_MS = 50;

function eqSpec(band: 'low' | 'mid' | 'high'): LaneSpec {
  return {
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

// A drag can scrub back and forth over the same time range; the last value
// written at each timestamp is the one the user ended on.
export function normalizeGestureSamples(samples: LanePoint[]): LanePoint[] {
  const byMs = new Map<number, number>();
  for (const sample of samples) byMs.set(sample.ms, sample.value);
  return [...byMs.entries()]
    .map(([ms, value]) => ({ ms, value }))
    .sort((first, second) => first.ms - second.ms);
}

export function decimateSteps(points: LanePoint[], epsilon: number): LanePoint[] {
  if (points.length <= 1) return [...points];
  const out: LanePoint[] = [points[0]];
  for (let pointIdx = 1; pointIdx < points.length - 1; pointIdx++) {
    if (Math.abs(points[pointIdx].value - out[out.length - 1].value) >= epsilon) {
      out.push(points[pointIdx]);
    }
  }
  const last = points[points.length - 1];
  if (last.value !== out[out.length - 1].value) out.push(last);
  return out;
}

export function originalValueAt(
  events: SessionEvent[],
  spec: LaneSpec,
  deck: string,
  ms: number
): number {
  for (let eventIdx = events.length - 1; eventIdx >= 0; eventIdx--) {
    const event = events[eventIdx];
    if (event.elapsed_ms > ms) continue;
    const value = spec.valueAt(event, deck);
    if (value !== undefined) return value;
  }
  return spec.defaultValue;
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
  if (points.length === 0 || t1 - t0 < MIN_GESTURE_MS) return events;

  const snap = spec.snap ?? ((value: number) => value);
  const clampValue = (value: number) => snap(Math.min(spec.max, Math.max(spec.min, value)));

  const kept = events.filter(
    (event) => !(event.elapsed_ms >= t0 && event.elapsed_ms <= t1 && spec.matches(event, deck))
  );

  const inserted = points.map((point) => spec.makeEvent(point.ms, clampValue(point.value), deck));

  const restoreValue = originalValueAt(events, spec, deck, t1);
  const lastDrawnValue = spec.valueAt(inserted[inserted.length - 1], deck);
  if (lastDrawnValue !== restoreValue) {
    inserted.push(spec.makeEvent(t1, restoreValue, deck));
  }

  // Stable sort keeps pre-existing events ahead of inserted ones at identical
  // timestamps, so an inserted value applied at the same ms wins.
  return [...kept, ...inserted].sort((first, second) => first.elapsed_ms - second.elapsed_ms);
}

export function filterActiveAt(
  events: SessionEvent[],
  deck: string,
  ms: number,
  inclusive = true
): boolean {
  for (let eventIdx = events.length - 1; eventIdx >= 0; eventIdx--) {
    const event = events[eventIdx];
    if (inclusive ? event.elapsed_ms > ms : event.elapsed_ms >= ms) continue;
    if (event.type === 'set_filter_active' && event.deck === deck && event.active !== undefined) {
      return event.active;
    }
  }
  return false;
}

// Shift+drag on the filter lane: toggles the filter's on/off state over [t0, t1]
// (on if it was off entering the range, off if it was on), independent of the
// knob curve. The original state at t1 is restored, same locality rule as
// value edits. Returns a new array; the input is never mutated.
export function toggleFilterActiveRange(
  events: SessionEvent[],
  deck: string,
  t0: number,
  t1: number
): SessionEvent[] {
  if (t1 - t0 < MIN_GESTURE_MS) return events;

  const want = !filterActiveAt(events, deck, t0, false);

  const kept = events.filter(
    (event) =>
      !(
        event.type === 'set_filter_active' &&
        event.deck === deck &&
        event.elapsed_ms >= t0 &&
        event.elapsed_ms <= t1
      )
  );

  const inserted: SessionEvent[] = [
    { elapsed_ms: t0, type: 'set_filter_active', deck, active: want }
  ];
  const restoreActive = filterActiveAt(events, deck, t1, true);
  if (restoreActive !== want) {
    inserted.push({ elapsed_ms: t1, type: 'set_filter_active', deck, active: restoreActive });
  }

  return [...kept, ...inserted].sort((first, second) => first.elapsed_ms - second.elapsed_ms);
}

export function nudgeValueAt(
  events: SessionEvent[],
  deck: string,
  ms: number,
  inclusive = true
): number {
  for (let eventIdx = events.length - 1; eventIdx >= 0; eventIdx--) {
    const event = events[eventIdx];
    if (inclusive ? event.elapsed_ms > ms : event.elapsed_ms >= ms) continue;
    if (event.type === 'set_nudge' && event.deck === deck && event.percent !== undefined) {
      return event.percent;
    }
  }
  return 0;
}

// Shift+drag on a deck's waveform row paints a nudge over [t0, t1]: top half of
// the row nudges up (positive percent), bottom half nudges down. The percent is
// the same nudge sensitivity used by the performance keys, so painted nudges
// sound identical to performed ones. The recorded value at t1 is restored
// (normally 0; the recorded percent when the range ends inside an active nudge).
// Returns a new array; the input is never mutated.
export function paintNudgeRange(
  events: SessionEvent[],
  deck: string,
  t0: number,
  t1: number,
  percent: number
): SessionEvent[] {
  if (t1 - t0 < MIN_GESTURE_MS) return events;

  const kept = events.filter(
    (event) =>
      !(
        event.type === 'set_nudge' &&
        event.deck === deck &&
        event.elapsed_ms >= t0 &&
        event.elapsed_ms <= t1
      )
  );

  const inserted: SessionEvent[] = [{ elapsed_ms: t0, type: 'set_nudge', deck, percent }];
  const restorePercent = nudgeValueAt(events, deck, t1, true);
  if (restorePercent !== percent) {
    inserted.push({ elapsed_ms: t1, type: 'set_nudge', deck, percent: restorePercent });
  }

  return [...kept, ...inserted].sort((first, second) => first.elapsed_ms - second.elapsed_ms);
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
