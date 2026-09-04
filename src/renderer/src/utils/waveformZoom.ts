export type SecondsView = { startSec: number; endSec: number };

export function clampedView(
  startSec: number,
  durationSec: number,
  trackDuration: number
): SecondsView {
  const duration = Math.min(durationSec, trackDuration);
  const start = Math.max(0, Math.min(startSec, trackDuration - duration));
  return { startSec: start, endSec: start + duration };
}

export function anchoredView(
  anchorSec: number,
  anchorFrac: number,
  durationSec: number,
  trackDuration: number
): SecondsView {
  return clampedView(anchorSec - anchorFrac * durationSec, durationSec, trackDuration);
}

// The view is clamped to the track, so a level wider than the track is not what is shown.
export function visibleSpanLabel(levelSec: number, trackDuration: number): string {
  const span = trackDuration > 0 ? Math.min(levelSec, trackDuration) : levelSec;
  if (span >= 60) return `${Math.round(span / 6) / 10}m`;
  if (span >= 1) return `${Math.round(span)}s`;
  return `${Math.round(span * 100) / 100}s`;
}
