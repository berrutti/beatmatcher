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
