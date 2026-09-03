import { describe, it, expect } from 'vitest';
import {
  stripColumnRate,
  stripScaleX,
  devicePixelsPerColumn,
  snappedToDevicePixel,
  stripX
} from '../stripGeometry';

const HALF_WINDOW_SEC = 5;

function ratioFor(canvasWidthDevicePx: number, dpr: number, rate: number): number {
  const columnRate = stripColumnRate(canvasWidthDevicePx, HALF_WINDOW_SEC);
  const scaleX = stripScaleX(canvasWidthDevicePx / dpr, HALF_WINDOW_SEC, columnRate, rate);
  return devicePixelsPerColumn(scaleX, dpr);
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
