import { describe, it, expect } from 'vitest';
import { thumbCentre, pressIsOnThumb, type ThumbGeometry } from '@renderer/utils/sliderThumb';

const HORIZONTAL: ThumbGeometry = {
  min: 0,
  max: 1,
  value: 0,
  trackLength: 100,
  thumbLength: 20,
  maxAtStart: false
};

const VERTICAL: ThumbGeometry = { ...HORIZONTAL, maxAtStart: true };

describe('thumbCentre', () => {
  it('keeps the thumb inside the track at both ends', () => {
    expect(thumbCentre({ ...HORIZONTAL, value: 0 })).toBe(10);
    expect(thumbCentre({ ...HORIZONTAL, value: 1 })).toBe(90);
  });

  it('travels backwards when the maximum is at the start', () => {
    expect(thumbCentre({ ...VERTICAL, value: 0 })).toBe(90);
    expect(thumbCentre({ ...VERTICAL, value: 1 })).toBe(10);
  });

  it('puts a centred value at the middle of the track', () => {
    expect(thumbCentre({ ...HORIZONTAL, value: 0.5 })).toBe(50);
    expect(thumbCentre({ ...VERTICAL, value: 0.5 })).toBe(50);
  });

  it('stays put for a range with no span, rather than dividing by zero', () => {
    expect(thumbCentre({ ...HORIZONTAL, min: 1, max: 1, value: 1 })).toBe(10);
  });

  it('never leaves the track for a value outside the range', () => {
    expect(thumbCentre({ ...HORIZONTAL, value: 5 })).toBe(90);
    expect(thumbCentre({ ...HORIZONTAL, value: -5 })).toBe(10);
  });

  it('centres the thumb when it is longer than the track', () => {
    expect(thumbCentre({ ...HORIZONTAL, trackLength: 10, thumbLength: 20, value: 1 })).toBe(10);
  });
});

describe('pressIsOnThumb', () => {
  const centred = { ...HORIZONTAL, value: 0.5 };

  it('takes a press anywhere inside the thumb', () => {
    expect(pressIsOnThumb(centred, 41, 0)).toBe(true);
    expect(pressIsOnThumb(centred, 59, 0)).toBe(true);
  });

  it('refuses a press past the far side of the track', () => {
    expect(pressIsOnThumb(centred, 5, 0)).toBe(false);
    expect(pressIsOnThumb(centred, 95, 0)).toBe(false);
  });

  it('grants only the grace it is given, because a bare thumb wants none', () => {
    expect(pressIsOnThumb(centred, 38, 0)).toBe(false);
    expect(pressIsOnThumb(centred, 38, 4)).toBe(true);
    expect(pressIsOnThumb(centred, 62, 4)).toBe(true);
  });

  it('tracks the thumb rather than the middle of the track', () => {
    const low = { ...HORIZONTAL, value: 0 };
    expect(pressIsOnThumb(low, 10, 0)).toBe(true);
    expect(pressIsOnThumb(low, 50, 0)).toBe(false);
  });
});
