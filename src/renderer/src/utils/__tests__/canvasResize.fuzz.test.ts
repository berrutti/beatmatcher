import { describe, it, expect } from 'vitest';
import { computeCanvasSize } from '../canvasResize';

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

describe('computeCanvasSize under fuzzed fractional CSS sizes', () => {
  it('always returns integer dimensions, even from fractional client sizes and dpr', () => {
    const random = makeRandom(7);
    for (let step = 0; step < 2000; step++) {
      const clientWidth = random() * 3000;
      const clientHeight = random() * 3000;
      const dpr = 0.5 + random() * 3;
      const size = computeCanvasSize(clientWidth, clientHeight, dpr);
      if (!size) continue;
      expect(Number.isInteger(size.width)).toBe(true);
      expect(Number.isInteger(size.height)).toBe(true);
    }
  });

  it('is idempotent for a fixed client size, so a per-frame resize check settles', () => {
    const random = makeRandom(11);
    for (let step = 0; step < 2000; step++) {
      const clientWidth = random() * 3000;
      const clientHeight = random() * 3000;
      const dpr = 0.5 + random() * 3;
      const first = computeCanvasSize(clientWidth, clientHeight, dpr);
      const second = computeCanvasSize(clientWidth, clientHeight, dpr);
      expect(second).toEqual(first);
    }
  });

  it('returns null for a zero-sized element instead of an integer zero mismatch', () => {
    expect(computeCanvasSize(0, 100, 2)).toBeNull();
    expect(computeCanvasSize(100, 0, 2)).toBeNull();
    expect(computeCanvasSize(0, 0, 2)).toBeNull();
  });

  it('rounds rather than truncates at the halfway point', () => {
    expect(computeCanvasSize(100.5, 50, 1)).toEqual({ width: 101, height: 50 });
  });
});
