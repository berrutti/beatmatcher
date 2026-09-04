import type { WaveformStyleOption } from '@renderer/utils/types';

export function stripColumnRate(canvasWidthDevicePx: number, halfWindowSec: number): number {
  return canvasWidthDevicePx / (2 * halfWindowSec);
}

export function stripScaleX(
  cssWidth: number,
  halfWindowSec: number,
  columnRate: number,
  rate: number
): number {
  return cssWidth / (2 * halfWindowSec * columnRate * rate);
}

export function snappedToDevicePixel(value: number, dpr: number): number {
  return Math.round(value * dpr) / dpr;
}

export function stripX(
  cssWidth: number,
  halfWindowSec: number,
  pos: number,
  rate: number,
  sec: number
): number {
  return cssWidth / 2 + (((sec - pos) / rate) * cssWidth) / (2 * halfWindowSec);
}

export type StripBuild = {
  builtFrom: Float32Array | null;
  builtPointsReady: number;
  displayRate: number;
  numSteps: number;
  bufferStartSec: number;
  lastBuiltMain: number;
  lastBuiltDpr: number;
  lastBuiltStyle: WaveformStyleOption | null;
  builtBandReference: number;
};

export type StripFrame = {
  data: Float32Array | null;
  pointsReady: number;
  position: number;
  cssWidth: number;
  dpr: number;
  style: WaveformStyleOption;
  bandReference: number;
  edgeGuardSec: number;
};

export function stripBitmapIsStale(built: StripBuild, frame: StripFrame): boolean {
  const bufferEndSec = built.bufferStartSec + built.numSteps / (built.displayRate || 1);
  return (
    built.builtFrom !== frame.data ||
    // The store fills one buffer in place, so the array is the same object after a
    // chunk of points arrives.
    built.builtPointsReady !== frame.pointsReady ||
    built.lastBuiltMain !== frame.cssWidth ||
    built.lastBuiltDpr !== frame.dpr ||
    built.lastBuiltStyle !== frame.style ||
    built.builtBandReference !== frame.bandReference ||
    frame.position < built.bufferStartSec + frame.edgeGuardSec ||
    frame.position > bufferEndSec - frame.edgeGuardSec
  );
}
