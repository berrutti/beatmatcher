// Pure view-window math for the session timeline's zoom/pan/scrub interactions.
// Kept free of canvas/DOM/reactive state so it can be unit tested directly:
// every function takes plain numbers and returns plain numbers/objects.

export type ViewWindow = { start: number; duration: number };

function finiteOr(value: number, fallback: number): number {
  return Number.isFinite(value) ? value : fallback;
}

// One NaN reaching the view blanks the entire timeline, because every scene
// item projects through it and a NaN coordinate draws nothing.
export function clampView(
  start: number,
  duration: number,
  totalMs: number,
  minViewMs: number
): ViewWindow {
  const total = Math.max(finiteOr(totalMs, minViewMs), minViewMs);
  const dur = Math.max(minViewMs, Math.min(finiteOr(duration, total), total));
  const start2 = Math.max(0, Math.min(finiteOr(start, 0), total - dur));
  return { start: start2, duration: dur };
}

export function zoomAroundCursor(
  view: ViewWindow,
  cursorFrac: number,
  deltaY: number,
  sensitivity: number,
  totalMs: number,
  minViewMs: number
): ViewWindow {
  const cursorMs = view.start + cursorFrac * view.duration;
  const factor = Math.exp(deltaY * sensitivity);
  const nextDuration = view.duration * factor;
  const nextStart = cursorMs - cursorFrac * nextDuration;
  return clampView(nextStart, nextDuration, totalMs, minViewMs);
}

export function panByMs(
  view: ViewWindow,
  deltaMs: number,
  totalMs: number,
  minViewMs: number
): ViewWindow {
  return clampView(view.start + deltaMs, view.duration, totalMs, minViewMs);
}

export function recenterOn(
  view: ViewWindow,
  centerMs: number,
  totalMs: number,
  minViewMs: number
): ViewWindow {
  return clampView(centerMs - view.duration / 2, view.duration, totalMs, minViewMs);
}

export function resizeLeftEdge(
  view: ViewWindow,
  newStartMs: number,
  totalMs: number,
  minViewMs: number
): ViewWindow {
  const end = view.start + view.duration;
  const start = Math.min(newStartMs, end - minViewMs);
  return clampView(start, end - start, totalMs, minViewMs);
}

export function resizeRightEdge(
  view: ViewWindow,
  newEndMs: number,
  totalMs: number,
  minViewMs: number
): ViewWindow {
  const start = view.start;
  const end = Math.max(newEndMs, start + minViewMs);
  return clampView(start, end - start, totalMs, minViewMs);
}

// Re-frames the view to keep `targetMs` visible, used to keep the playhead on
// screen: jumps so the target sits a fixed lead-in margin from the left edge,
// rather than smoothly scrolling each frame.
export function followTarget(
  view: ViewWindow,
  targetMs: number,
  leadInFraction: number,
  totalMs: number,
  minViewMs: number
): ViewWindow | null {
  if (targetMs >= view.start && targetMs <= view.start + view.duration) return null;
  return clampView(targetMs - view.duration * leadInFraction, view.duration, totalMs, minViewMs);
}

export function overlapsRange(startA: number, endA: number, startB: number, endB: number): boolean {
  return endA >= startB && startA <= endB;
}

// Returns the minimal contiguous slice of a ms-sorted array of points that
// covers the view window, including one boundary point on each side for
// step-graph continuity. Uses binary search: O(log n) + O(visible count).
export function sliceVisiblePoints<T extends { ms: number }>(
  points: T[],
  viewStart: number,
  viewEnd: number
): T[] {
  if (points.length === 0) return points;

  let searchLo = 0;
  let searchHi = points.length;
  while (searchLo < searchHi) {
    const mid = (searchLo + searchHi) >>> 1;
    if (points[mid].ms <= viewStart) searchLo = mid + 1;
    else searchHi = mid;
  }
  const startIdx = Math.max(0, searchLo - 1);

  searchLo = 0;
  searchHi = points.length;
  while (searchLo < searchHi) {
    const mid = (searchLo + searchHi) >>> 1;
    if (points[mid].ms < viewEnd) searchLo = mid + 1;
    else searchHi = mid;
  }
  const endIdx = Math.min(points.length, searchLo + 1);

  return points.slice(startIdx, endIdx);
}

export function msToFrac(ms: number, view: ViewWindow): number {
  return (ms - view.start) / (view.duration || 1);
}

export function fracToMs(frac: number, view: ViewWindow): number {
  return view.start + frac * view.duration;
}

export function clampFrac(frac: number): number {
  return Math.max(0, Math.min(1, frac));
}

export const OVERVIEW_PARTS = ['resize-left', 'resize-right', 'move', 'outside'] as const;

export type OverviewHit = (typeof OVERVIEW_PARTS)[number];

// `frac` and `edgeTolerance` are fractions of the full session duration (0..1),
// matching how the overview strip maps the whole session across its width.
export function hitTestOverview(
  frac: number,
  view: ViewWindow,
  totalMs: number,
  edgeTolerance: number
): OverviewHit {
  const total = totalMs || 1;
  const startFrac = view.start / total;
  const endFrac = (view.start + view.duration) / total;
  if (Math.abs(frac - startFrac) <= edgeTolerance) return 'resize-left';
  if (Math.abs(frac - endFrac) <= edgeTolerance) return 'resize-right';
  if (frac >= startFrac && frac <= endFrac) return 'move';
  return 'outside';
}

export function chooseTickInterval(viewDurationMs: number, availPx: number): number {
  const candidates = [
    50, 100, 200, 500, 1000, 2000, 5000, 10000, 15000, 30000, 60000, 120000, 300000, 600000
  ];
  const minGapPx = 60;
  for (const ms of candidates) {
    if ((ms / viewDurationMs) * availPx >= minGapPx) return ms;
  }
  return candidates[candidates.length - 1];
}
