import { describe, it, expect } from 'vitest';
import { spectralColor } from '../waveformImage';

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

// Band values are multiples of the band's own track RMS, so they run well past 1.
function randomBands(random: () => number): [number, number, number] {
  return [random() * 8, random() * 8, random() * 8];
}

describe('spectralColor under fuzzed band balances', () => {
  it('always returns three integers inside 0..255', () => {
    const random = makeRandom(3);
    for (let step = 0; step < 4000; step++) {
      const [bass, mid, high] = randomBands(random);
      for (const channel of spectralColor(bass, mid, high, random() * 1.5)) {
        expect(Number.isInteger(channel)).toBe(true);
        expect(channel).toBeGreaterThanOrEqual(0);
        expect(channel).toBeLessThanOrEqual(255);
      }
    }
  });

  it('puts the strongest band on the strongest channel', () => {
    const random = makeRandom(5);
    for (let step = 0; step < 4000; step++) {
      const bands = randomBands(random);
      const color = spectralColor(bands[0], bands[1], bands[2], random());
      const loudestBand = bands.indexOf(Math.max(...bands));
      expect(color[loudestBand]).toBe(Math.max(...color));
    }
  });

  it('holds the balance between channels as amplitude changes', () => {
    const random = makeRandom(13);
    for (let step = 0; step < 2000; step++) {
      const [bass, mid, high] = randomBands(random);
      const loud = spectralColor(bass, mid, high, 1);
      const quiet = spectralColor(bass, mid, high, 0.2);
      for (let channel = 0; channel < 3; channel++) {
        const loudShare = loud[channel] / Math.max(...loud);
        const quietShare = quiet[channel] / Math.max(...quiet);
        expect(Math.abs(quietShare - loudShare)).toBeLessThan(0.05);
      }
    }
  });

  it('never brightens as amplitude falls', () => {
    const random = makeRandom(17);
    for (let step = 0; step < 2000; step++) {
      const [bass, mid, high] = randomBands(random);
      const amp = random();
      const brighter = spectralColor(bass, mid, high, amp);
      const dimmer = spectralColor(bass, mid, high, amp * random());
      for (let channel = 0; channel < 3; channel++) {
        expect(dimmer[channel]).toBeLessThanOrEqual(brighter[channel]);
      }
    }
  });

  it('survives amplitudes outside 0..1 and denormal bands', () => {
    const random = makeRandom(19);
    for (let step = 0; step < 2000; step++) {
      const [bass, mid, high] = randomBands(random);
      for (const amp of [-1, 0, 1e-320, 1, 4, Number.EPSILON]) {
        for (const channel of spectralColor(bass * 1e-300, mid, high, amp)) {
          expect(Number.isFinite(channel)).toBe(true);
        }
      }
    }
  });
});
