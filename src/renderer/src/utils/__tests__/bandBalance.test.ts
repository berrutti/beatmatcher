import { describe, it, expect } from 'vitest';
import { addBandSquares, bandBalanceOf, densePointsFit, type BandSquares } from '../bandBalance';

function pointsOf(bands: [number, number, number][]): Float32Array {
  const out = new Float32Array(bands.length * 4);
  bands.forEach(([bass, mid, high], index) => {
    out.set([bass, mid, high, 1], index * 4);
  });
  return out;
}

describe('bandBalanceOf', () => {
  it('is flat before any point has arrived', () => {
    expect(bandBalanceOf([0, 0, 0], 0)).toEqual([1, 1, 1]);
  });

  it('lifts each band to the three levels in quadrature', () => {
    const balance = bandBalanceOf([0.16, 0.04, 0.01], 1);
    const reference = Math.sqrt(0.16 + 0.04 + 0.01);
    expect(balance[0]).toBeCloseTo(reference / 0.4, 6);
    expect(balance[1]).toBeCloseTo(reference / 0.2, 6);
    expect(balance[2]).toBeCloseTo(reference / 0.1, 6);
  });

  it('leaves a silent band alone rather than dividing by zero', () => {
    expect(bandBalanceOf([0.16, 0, 0.01], 1)[1]).toBe(1);
  });

  it('is not flat once a bass-heavy first chunk has arrived', () => {
    const squares: BandSquares = [0, 0, 0];
    addBandSquares(squares, pointsOf([[0.9, 0.4, 0.15]]), 1);
    const balance = bandBalanceOf(squares, 1);
    expect(balance[0]).toBeLessThan(balance[1]);
    expect(balance[1]).toBeLessThan(balance[2]);
  });

  it('reaches the same balance whether the points arrive at once or in chunks', () => {
    const bands: [number, number, number][] = [
      [0.9, 0.4, 0.15],
      [0.2, 0.5, 0.3],
      [1.4, 0.3, 0.1],
      [0.05, 0.1, 0.6]
    ];
    const whole: BandSquares = [0, 0, 0];
    addBandSquares(whole, pointsOf(bands), bands.length);

    const streamed: BandSquares = [0, 0, 0];
    for (const band of bands) addBandSquares(streamed, pointsOf([band]), 1);

    expect(bandBalanceOf(streamed, bands.length)).toEqual(bandBalanceOf(whole, bands.length));
  });
});

describe('densePointsFit', () => {
  it('accepts a chunk that lands inside the buffer', () => {
    expect(densePointsFit(new Float32Array(40), 0, 10)).toBe(true);
    expect(densePointsFit(new Float32Array(40), 5, 5)).toBe(true);
  });

  it('refuses a chunk that would run past a buffer sized for a shorter track', () => {
    expect(densePointsFit(new Float32Array(40), 8, 5)).toBe(false);
    expect(densePointsFit(new Float32Array(40), 0, 11)).toBe(false);
  });

  it('refuses anything before a buffer exists', () => {
    expect(densePointsFit(null, 0, 1)).toBe(false);
  });
});

describe('the reference matches the Rust band_reference fixture', () => {
  it('lifts 0.4, 0.2 and 0.1 to the shared values', () => {
    const balance = bandBalanceOf([0.16, 0.04, 0.01], 1);
    expect(balance[0]).toBeCloseTo(1.14564392, 6);
    expect(balance[1]).toBeCloseTo(2.29128785, 6);
    expect(balance[2]).toBeCloseTo(4.58257569, 6);
  });
});
