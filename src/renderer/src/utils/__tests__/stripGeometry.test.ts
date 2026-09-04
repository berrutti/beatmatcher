import { describe, it, expect } from 'vitest';
import {
  stripColumnRate,
  stripScaleX,
  snappedToDevicePixel,
  stripX,
  stripBitmapIsStale,
  type StripBuild,
  type StripFrame
} from '../stripGeometry';

const HALF_WINDOW_SEC = 5;

function ratioFor(canvasWidthDevicePx: number, dpr: number, rate: number): number {
  const columnRate = stripColumnRate(canvasWidthDevicePx, HALF_WINDOW_SEC);
  const scaleX = stripScaleX(canvasWidthDevicePx / dpr, HALF_WINDOW_SEC, columnRate, rate);
  return scaleX * dpr;
}

describe('the strip draws one source column per device pixel', () => {
  it('holds at every canvas width and device pixel ratio', () => {
    for (const dpr of [1, 1.5, 2, 3]) {
      for (const cssWidth of [320, 690, 689.5, 1400, 1920]) {
        const canvasWidth = Math.round(cssWidth * dpr);
        expect(ratioFor(canvasWidth, dpr, 1)).toBeCloseTo(1, 10);
      }
    }
  });

  it('falls to 1/rate on a pitched deck, which is the known limit', () => {
    expect(ratioFor(1380, 2, 1.06)).toBeCloseTo(1 / 1.06, 10);
    expect(ratioFor(1380, 2, 0.92)).toBeCloseTo(1 / 0.92, 10);
  });
});

describe('stripColumnRate', () => {
  it('counts physical pixels, so a retina strip holds twice the columns', () => {
    expect(stripColumnRate(1380, HALF_WINDOW_SEC)).toBe(2 * stripColumnRate(690, HALF_WINDOW_SEC));
  });

  it('fits the window across the canvas', () => {
    expect(stripColumnRate(1380, HALF_WINDOW_SEC) * (2 * HALF_WINDOW_SEC)).toBe(1380);
  });
});

describe('snappedToDevicePixel', () => {
  it('lands on a whole device pixel, so a scroll cannot translate by a fraction', () => {
    for (const dpr of [1, 2, 3]) {
      for (const value of [0, 1.4, 12.7, -3.2, 100.5]) {
        expect(Number.isInteger(snappedToDevicePixel(value, dpr) * dpr)).toBe(true);
      }
    }
  });

  it('leaves an already-snapped value alone', () => {
    const snapped = snappedToDevicePixel(12.7, 2);
    expect(snappedToDevicePixel(snapped, 2)).toBe(snapped);
  });
});

describe('stripX', () => {
  it('puts the playhead position at the centre', () => {
    expect(stripX(690, HALF_WINDOW_SEC, 30, 1, 30)).toBe(345);
  });

  it('puts the window edges at the canvas edges', () => {
    expect(stripX(690, HALF_WINDOW_SEC, 30, 1, 25)).toBe(0);
    expect(stripX(690, HALF_WINDOW_SEC, 30, 1, 35)).toBe(690);
  });

  it('fits fewer audio seconds when the deck is pitched up', () => {
    expect(stripX(690, HALF_WINDOW_SEC, 30, 2, 35)).toBe(517.5);
  });
});

describe('stripBitmapIsStale', () => {
  const points = new Float32Array(150 * 60 * 4);

  const built: StripBuild = {
    builtFrom: points,
    builtPointsReady: 1500,
    displayRate: 138,
    numSteps: 138 * 40,
    bufferStartSec: 10,
    lastBuiltMain: 690,
    lastBuiltDpr: 2,
    lastBuiltStyle: 'threeBand',
    builtBandReference: 0.3
  };

  const frame: StripFrame = {
    data: points,
    pointsReady: 1500,
    position: 30,
    cssWidth: 690,
    dpr: 2,
    style: 'threeBand',
    bandReference: 0.3,
    edgeGuardSec: HALF_WINDOW_SEC + 5
  };

  it('keeps the bitmap when nothing has moved', () => {
    expect(stripBitmapIsStale(built, frame)).toBe(false);
  });

  it('rebuilds when points landed in the buffer it was built from', () => {
    expect(stripBitmapIsStale(built, { ...frame, pointsReady: 2400 })).toBe(true);
  });

  it('rebuilds when the track reference settles on the one the bands measured', () => {
    expect(stripBitmapIsStale(built, { ...frame, bandReference: 0.31 })).toBe(true);
  });

  it('rebuilds on a new track, a resize, a device pixel ratio change and a new style', () => {
    expect(stripBitmapIsStale(built, { ...frame, data: new Float32Array(4) })).toBe(true);
    expect(stripBitmapIsStale(built, { ...frame, cssWidth: 700 })).toBe(true);
    expect(stripBitmapIsStale(built, { ...frame, dpr: 1 })).toBe(true);
    expect(stripBitmapIsStale(built, { ...frame, style: 'blended' })).toBe(true);
  });

  it('rebuilds once the playhead comes within the guard of either buffer edge', () => {
    expect(stripBitmapIsStale(built, { ...frame, position: 19 })).toBe(true);
    expect(stripBitmapIsStale(built, { ...frame, position: 41 })).toBe(true);
    expect(stripBitmapIsStale(built, { ...frame, position: 21 })).toBe(false);
    expect(stripBitmapIsStale(built, { ...frame, position: 39 })).toBe(false);
  });
});
