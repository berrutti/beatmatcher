import type { SessionEvent, EditableLaneKey, LanePoint } from '@renderer/utils/types';
import {
  laneSpecs,
  laneMoveSpan as coreLaneMoveSpan,
  resetLaneFrom as coreResetLaneFrom,
  type LaneMoveSpan,
  type ResetExtent,
  spliceLaneEvents as coreSpliceLaneEvents,
  type LaneSpec
} from '@renderer/utils/sessionCore';

// Rate is the one lane whose range depends on the loaded track, not the lane.
export function laneSpecFor(
  key: EditableLaneKey,
  mixerId: string,
  opts: { rateMin?: number; rateMax?: number } = {}
): LaneSpec {
  const spec = laneSpecs(mixerId)[key];
  if (key !== 'rate') return spec;
  return { ...spec, min: opts.rateMin ?? spec.min, max: opts.rateMax ?? spec.max };
}

// Restores the original value at t1, so everything after the gesture sounds
// unchanged.
export function spliceLaneEvents(
  events: SessionEvent[],
  spec: LaneSpec,
  mixerId: string,
  deck: string,
  t0: number,
  t1: number,
  points: LanePoint[]
): SessionEvent[] {
  return coreSpliceLaneEvents(events, spec.key, mixerId, deck, t0, t1, points, spec.min, spec.max);
}

export function resetLaneFrom(
  events: SessionEvent[],
  spec: LaneSpec,
  mixerId: string,
  deck: string,
  ms: number,
  extent: ResetExtent
): SessionEvent[] {
  return coreResetLaneFrom(events, spec.key, mixerId, deck, ms, extent, spec.min, spec.max);
}

export function laneMoveSpan(
  events: SessionEvent[],
  spec: LaneSpec,
  mixerId: string,
  deck: string,
  ms: number
): LaneMoveSpan | null {
  return coreLaneMoveSpan(events, spec.key, mixerId, deck, ms, spec.min, spec.max);
}

// Driven by the lane's unit rather than its key: the same eq lane reads in dB
// on the classic mixer and as a 0-1 kill on the isolator.
export function formatLaneValue(spec: LaneSpec, value: number): string {
  switch (spec.unit) {
    case 'db':
      return `${value > 0 ? '+' : ''}${value.toFixed(1)} dB`;
    case 'ratio': {
      const pct = (value - 1) * 100;
      return `${pct > 0 ? '+' : ''}${pct.toFixed(1)}%`;
    }
    case 'bool':
      return value === 0 ? 'off' : 'on';
    case 'normalized':
      return value.toFixed(2);
    case 'percent':
      return `${value > 0 ? '+' : ''}${value.toFixed(1)}%`;
  }
}
