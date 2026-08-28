import { describe, it, expect } from 'vitest';
import { thumbCentre, pressIsOnThumb, type ThumbGeometry } from '@renderer/utils/sliderThumb';

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

// Every shape the app actually declares, plus ranges that invert, sit off zero
// or collapse to a point.
function fuzzGeometry(random: () => number): ThumbGeometry {
  const min = (random() - 0.5) * 200;
  const max = min + random() * 200;
  const trackLength = random() * 400;
  return {
    min,
    max,
    value: min + random() * (max - min),
    trackLength,
    thumbLength: random() * 60,
    maxAtStart: random() < 0.5
  };
}

describe('thumb geometry under fuzzed sliders', () => {
  it('never places the thumb outside the track it rides', () => {
    const random = makeRandom(7);
    for (let step = 0; step < 5000; step++) {
      const geometry = fuzzGeometry(random);
      const centre = thumbCentre(geometry);
      const half = geometry.thumbLength / 2;

      expect(centre, `${step}`).toBeGreaterThanOrEqual(half - 1e-9);
      expect(centre, `${step}`).toBeLessThanOrEqual(
        Math.max(half, geometry.trackLength - half) + 1e-9
      );
    }
  });

  it('always answers yes for a press on the thumb, at any value', () => {
    const random = makeRandom(11);
    for (let step = 0; step < 5000; step++) {
      const geometry = fuzzGeometry(random);
      const centre = thumbCentre(geometry);
      const grace = random() * 8;

      expect(pressIsOnThumb(geometry, centre, grace), `${step}`).toBe(true);
    }
  });

  it('answers no just outside the thumb and yes just inside, at any value', () => {
    const random = makeRandom(13);
    for (let step = 0; step < 5000; step++) {
      const geometry = fuzzGeometry(random);
      if (geometry.thumbLength < 2) continue;
      const centre = thumbCentre(geometry);
      const grace = random() * 8;
      const edge = geometry.thumbLength / 2 + grace;

      expect(pressIsOnThumb(geometry, centre + edge - 0.01, grace), `${step}`).toBe(true);
      expect(pressIsOnThumb(geometry, centre + edge + 0.01, grace), `${step}`).toBe(false);
      expect(pressIsOnThumb(geometry, centre - edge + 0.01, grace), `${step}`).toBe(true);
      expect(pressIsOnThumb(geometry, centre - edge - 0.01, grace), `${step}`).toBe(false);
    }
  });

  it('never widens the hit box below the grace it was given', () => {
    const random = makeRandom(17);
    for (let step = 0; step < 5000; step++) {
      const geometry = fuzzGeometry(random);
      const centre = thumbCentre(geometry);
      const tight = pressIsOnThumb(geometry, centre + geometry.thumbLength, 0);
      const loose = pressIsOnThumb(geometry, centre + geometry.thumbLength, 8);

      // Grace only ever adds reach, so a press the bare box took is never lost.
      expect(!tight || loose, `${step}`).toBe(true);
    }
  });

  it('moves the thumb one way as the value rises, whichever end the maximum is', () => {
    const random = makeRandom(19);
    for (let step = 0; step < 3000; step++) {
      const geometry = fuzzGeometry(random);
      if (geometry.max - geometry.min < 1e-6) continue;
      if (geometry.trackLength <= geometry.thumbLength) continue;
      const lower = geometry.min + (geometry.max - geometry.min) * 0.25;
      const higher = geometry.min + (geometry.max - geometry.min) * 0.75;

      const atLower = thumbCentre({ ...geometry, value: lower });
      const atHigher = thumbCentre({ ...geometry, value: higher });

      if (geometry.maxAtStart) expect(atHigher, `${step}`).toBeLessThan(atLower);
      else expect(atHigher, `${step}`).toBeGreaterThan(atLower);
    }
  });

  it('parks a range with no span at its minimum, rather than dividing by zero', () => {
    const random = makeRandom(23);
    for (let step = 0; step < 2000; step++) {
      const geometry = fuzzGeometry(random);
      const point = (random() - 0.5) * 200;
      const collapsed = { ...geometry, min: point, max: point, value: point };
      const half = collapsed.thumbLength / 2;
      // Which end that is depends on the orientation: a vertical fader carries
      // its minimum at the bottom.
      const atMinimum = collapsed.maxAtStart ? Math.max(half, collapsed.trackLength - half) : half;

      expect(Number.isFinite(thumbCentre(collapsed)), `${step}`).toBe(true);
      expect(thumbCentre(collapsed), `${step}`).toBeCloseTo(atMinimum, 9);
    }
  });

  it('keeps a value outside the range pinned to the end it passed', () => {
    const random = makeRandom(29);
    for (let step = 0; step < 3000; step++) {
      const geometry = fuzzGeometry(random);
      if (geometry.max - geometry.min < 1e-6) continue;
      const span = geometry.max - geometry.min;

      const past = thumbCentre({ ...geometry, value: geometry.max + span });
      const under = thumbCentre({ ...geometry, value: geometry.min - span });
      const atMax = thumbCentre({ ...geometry, value: geometry.max });
      const atMin = thumbCentre({ ...geometry, value: geometry.min });

      expect(past, `${step}`).toBeCloseTo(atMax, 9);
      expect(under, `${step}`).toBeCloseTo(atMin, 9);
    }
  });
});
