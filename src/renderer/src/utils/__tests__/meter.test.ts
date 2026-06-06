import { describe, it, expect } from 'vitest';
import { vuParam, smoothParam, stepPeak } from '../meter';

describe('vuParam', () => {
  it('maps silence to 0', () => {
    expect(vuParam(0)).toBe(0);
  });

  it('clamps full scale to 1', () => {
    expect(vuParam(1)).toBe(1);
  });

  it('clamps above full scale to 1', () => {
    expect(vuParam(2)).toBe(1);
  });

  it('maps a typical loud track (~-9 dBFS mean abs) to the green zone', () => {
    const p = vuParam(0.35);
    expect(p).toBeGreaterThan(0.5);
    expect(p).toBeLessThan(0.75);
  });

  it('maps a very hot signal (~-2 dBFS mean abs) to the yellow/red border', () => {
    const p = vuParam(0.77);
    expect(p).toBeGreaterThan(0.85);
    expect(p).toBeLessThan(1);
  });

  it('is monotonically increasing', () => {
    expect(vuParam(0.1)).toBeLessThan(vuParam(0.2));
    expect(vuParam(0.2)).toBeLessThan(vuParam(0.5));
    expect(vuParam(0.5)).toBeLessThan(vuParam(0.9));
  });
});

describe('smoothParam', () => {
  it('attacks immediately. jumps straight to a higher value', () => {
    expect(smoothParam(0.3, 0.8)).toBe(0.8);
    expect(smoothParam(0, 1)).toBe(1);
  });

  it('decays by 10% of the gap per tick', () => {
    const result = smoothParam(1.0, 0.0);
    expect(result).toBeCloseTo(0.9, 5);
  });

  it('is stable when current equals next', () => {
    expect(smoothParam(0.5, 0.5)).toBe(0.5);
  });

  it('never decays below next', () => {
    expect(smoothParam(0.2, 0.18)).toBeGreaterThanOrEqual(0.18);
  });
});

describe('stepPeak', () => {
  it('snaps up and resets hold timer when new param exceeds current peak', () => {
    const next = stepPeak({ value: 0.4, holdMs: 0 }, 0.9);
    expect(next.value).toBe(0.9);
    expect(next.holdMs).toBe(400);
  });

  it('holds the peak value during the hold period', () => {
    const next = stepPeak({ value: 0.9, holdMs: 200 }, 0.3);
    expect(next.value).toBe(0.9);
    expect(next.holdMs).toBe(167);
  });

  it('starts falling after hold expires', () => {
    const next = stepPeak({ value: 0.9, holdMs: 0 }, 0.3);
    expect(next.value).toBeCloseTo(0.878, 3);
    expect(next.holdMs).toBe(0);
  });

  it('does not fall below zero', () => {
    const next = stepPeak({ value: 0.01, holdMs: 0 }, 0);
    expect(next.value).toBeGreaterThanOrEqual(0);
  });

  it('does not mutate the input state', () => {
    const pk = { value: 0.8, holdMs: 100 };
    stepPeak(pk, 0.3);
    expect(pk.value).toBe(0.8);
    expect(pk.holdMs).toBe(100);
  });
});
