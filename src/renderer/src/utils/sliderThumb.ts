// Where a range input's thumb sits along its track, in the track's own axis.
// The caller resolves that axis from the DOM; this only does the arithmetic.
export type ThumbGeometry = {
  min: number;
  max: number;
  value: number;
  trackLength: number;
  thumbLength: number;
  // A vertical fader puts its maximum at the top, so the thumb travels back
  // along the track as the value rises.
  maxAtStart: boolean;
};

export function thumbCentre(geometry: ThumbGeometry): number {
  const { min, max, value, trackLength, thumbLength, maxAtStart } = geometry;
  const span = max - min;
  const fraction = span === 0 ? 0 : (value - min) / span;
  const travel = Math.max(0, trackLength - thumbLength);
  const along = maxAtStart ? 1 - fraction : fraction;
  return thumbLength / 2 + Math.min(1, Math.max(0, along)) * travel;
}

// `gracePx` covers what the thumb paints outside its box: a shadow reads as part
// of the cap. A thumb that paints nothing outside itself wants none.
export function pressIsOnThumb(geometry: ThumbGeometry, alongPx: number, gracePx: number): boolean {
  return Math.abs(alongPx - thumbCentre(geometry)) <= geometry.thumbLength / 2 + gracePx;
}
