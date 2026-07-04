import type { SessionEvent, EditableLaneKey, LanePoint } from '@renderer/utils/types';
import {
  DEFAULT_GAIN,
  DEFAULT_EQ_DB,
  DEFAULT_FILTER_VALUE,
  DEFAULT_RATE
} from '@renderer/composables/useSessionTimeline';
import { EQ_MIN_DB, EQ_MAX_DB, DEFAULT_MASTER_GAIN } from '@renderer/stores/mixer';
import { spliceLaneEvents as coreSpliceLaneEvents } from '@renderer/utils/sessionCore';

// Display metadata only; the event logic lives in session-core (lane_edit.rs).
export type LaneSpec = {
  key: EditableLaneKey;
  min: number;
  max: number;
  defaultValue: number;
  epsilon: number;
};

export function laneSpecFor(
  key: EditableLaneKey,
  opts: { rateMin?: number; rateMax?: number } = {}
): LaneSpec {
  switch (key) {
    case 'gain':
      return { key, min: 0, max: 1, defaultValue: DEFAULT_GAIN, epsilon: 0.01 };
    case 'eqLow':
    case 'eqMid':
    case 'eqHigh':
      return { key, min: EQ_MIN_DB, max: EQ_MAX_DB, defaultValue: DEFAULT_EQ_DB, epsilon: 0.25 };
    case 'filter':
      return { key, min: -1, max: 1, defaultValue: DEFAULT_FILTER_VALUE, epsilon: 0.01 };
    case 'rate':
      return {
        key,
        min: opts.rateMin ?? 0.92,
        max: opts.rateMax ?? 1.08,
        defaultValue: DEFAULT_RATE,
        epsilon: 0.0005
      };
    case 'masterGain':
      return { key, min: 0, max: 1, defaultValue: DEFAULT_MASTER_GAIN, epsilon: 0.01 };
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
