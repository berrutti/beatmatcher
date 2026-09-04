// log10(9x + 1) compresses loud signals and expands quiet ones.
const VU_LOG_SCALE = 9;

// Attack is instant.
const METER_FALL_COEFF = 0.1;

const PEAK_HOLD_MS = 400;
const PEAK_FRAME_MS = 33;
const PEAK_DECAY_PER_FRAME = 0.022;

export function vuParam(meanAbs: number): number {
  return Math.min(1, Math.log10(VU_LOG_SCALE * meanAbs + 1));
}

export function smoothParam(current: number, next: number): number {
  if (next >= current) return next;
  return current - METER_FALL_COEFF * (current - next);
}

export type PeakState = { value: number; holdMs: number };

export function stepPeak(pk: PeakState, newParam: number): PeakState {
  if (newParam >= pk.value) return { value: newParam, holdMs: PEAK_HOLD_MS };
  if (pk.holdMs > 0) return { value: pk.value, holdMs: pk.holdMs - PEAK_FRAME_MS };
  return { value: Math.max(0, pk.value - PEAK_DECAY_PER_FRAME), holdMs: 0 };
}
