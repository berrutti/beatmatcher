import { describe, it, expect } from 'vitest';
import {
  clampView,
  zoomAroundCursor,
  panByMs,
  recenterOn,
  resizeLeftEdge,
  resizeRightEdge,
  followTarget,
  overlapsRange,
  msToFrac,
  fracToMs,
  hitTestOverview,
  chooseTickInterval,
  sliceVisiblePoints,
  type ViewWindow
} from '../timelineView';

const MIN = 200;
const TOTAL = 100_000;

describe('clampView', () => {
  it('keeps a window that already fits within bounds', () => {
    expect(clampView(1000, 5000, TOTAL, MIN)).toEqual({ start: 1000, duration: 5000 });
  });

  it('clamps duration to the minimum', () => {
    expect(clampView(0, 50, TOTAL, MIN)).toEqual({ start: 0, duration: MIN });
  });

  it('clamps duration to the total session length', () => {
    expect(clampView(0, TOTAL * 2, TOTAL, MIN)).toEqual({ start: 0, duration: TOTAL });
  });

  it('clamps start so the window does not run past the end', () => {
    expect(clampView(TOTAL, 1000, TOTAL, MIN)).toEqual({ start: TOTAL - 1000, duration: 1000 });
  });

  it('clamps a negative start to zero', () => {
    expect(clampView(-5000, 1000, TOTAL, MIN)).toEqual({ start: 0, duration: 1000 });
  });

  it('grows total to at least minViewMs for very short sessions', () => {
    expect(clampView(0, 100, 100, MIN)).toEqual({ start: 0, duration: MIN });
  });
});

describe('zoomAroundCursor', () => {
  const view: ViewWindow = { start: 10_000, duration: 10_000 };

  it('zooms in (negative deltaY) and keeps the cursor position fixed', () => {
    const cursorFrac = 0.5; // cursor sits at ms = 15_000
    const next = zoomAroundCursor(view, cursorFrac, -500, 0.0015, TOTAL, MIN);
    expect(next.duration).toBeLessThan(view.duration);
    const cursorMsAfter = next.start + cursorFrac * next.duration;
    expect(cursorMsAfter).toBeCloseTo(15_000, 6);
  });

  it('zooms out (positive deltaY) and keeps the cursor position fixed', () => {
    const cursorFrac = 0.25; // cursor sits at ms = 12_500
    const next = zoomAroundCursor(view, cursorFrac, 500, 0.0015, TOTAL, MIN);
    expect(next.duration).toBeGreaterThan(view.duration);
    const cursorMsAfter = next.start + cursorFrac * next.duration;
    expect(cursorMsAfter).toBeCloseTo(12_500, 6);
  });

  it('never zooms in past the minimum view duration', () => {
    const next = zoomAroundCursor(view, 0.5, -100_000, 0.0015, TOTAL, MIN);
    expect(next.duration).toBe(MIN);
  });

  it('never zooms out past the full session duration', () => {
    const next = zoomAroundCursor(view, 0.5, 100_000, 0.0015, TOTAL, MIN);
    expect(next.duration).toBe(TOTAL);
  });
});

describe('panByMs', () => {
  it('shifts the window by the given delta', () => {
    expect(panByMs({ start: 1000, duration: 2000 }, 500, TOTAL, MIN)).toEqual({
      start: 1500,
      duration: 2000
    });
  });

  it('does not let the window run past the start of the session', () => {
    expect(panByMs({ start: 100, duration: 2000 }, -1000, TOTAL, MIN)).toEqual({
      start: 0,
      duration: 2000
    });
  });

  it('does not let the window run past the end of the session', () => {
    const view = { start: TOTAL - 2000, duration: 2000 };
    expect(panByMs(view, 5000, TOTAL, MIN)).toEqual({ start: TOTAL - 2000, duration: 2000 });
  });
});

describe('recenterOn', () => {
  it('centers the view on the target ms, keeping the duration', () => {
    const next = recenterOn({ start: 0, duration: 2000 }, 50_000, TOTAL, MIN);
    expect(next).toEqual({ start: 49_000, duration: 2000 });
  });

  it('clamps when centering near the start of the session', () => {
    const next = recenterOn({ start: 0, duration: 2000 }, 200, TOTAL, MIN);
    expect(next.start).toBe(0);
  });

  it('clamps when centering near the end of the session', () => {
    const next = recenterOn({ start: 0, duration: 2000 }, TOTAL, TOTAL, MIN);
    expect(next.start).toBe(TOTAL - 2000);
  });
});

describe('resizeLeftEdge', () => {
  const view: ViewWindow = { start: 10_000, duration: 10_000 };

  it('moves the start while keeping the end edge fixed', () => {
    const next = resizeLeftEdge(view, 12_000, TOTAL, MIN);
    expect(next).toEqual({ start: 12_000, duration: 8000 });
  });

  it('widening the window to the left grows the duration', () => {
    const next = resizeLeftEdge(view, 5000, TOTAL, MIN);
    expect(next).toEqual({ start: 5000, duration: 15_000 });
  });

  it('does not allow the duration to shrink below the minimum', () => {
    const next = resizeLeftEdge(view, 19_999, TOTAL, MIN);
    expect(next.duration).toBe(MIN);
    expect(next.start + next.duration).toBe(view.start + view.duration);
  });
});

describe('resizeRightEdge', () => {
  const view: ViewWindow = { start: 10_000, duration: 10_000 };

  it('moves the end while keeping the start edge fixed', () => {
    const next = resizeRightEdge(view, 18_000, TOTAL, MIN);
    expect(next).toEqual({ start: 10_000, duration: 8000 });
  });

  it('widening the window to the right grows the duration', () => {
    const next = resizeRightEdge(view, 25_000, TOTAL, MIN);
    expect(next).toEqual({ start: 10_000, duration: 15_000 });
  });

  it('does not allow the duration to shrink below the minimum', () => {
    const next = resizeRightEdge(view, 10_001, TOTAL, MIN);
    expect(next.duration).toBe(MIN);
    expect(next.start).toBe(view.start);
  });
});

describe('followTarget', () => {
  const view: ViewWindow = { start: 10_000, duration: 10_000 };

  it('returns null when the target is already inside the view', () => {
    expect(followTarget(view, 15_000, 0.1, TOTAL, MIN)).toBeNull();
  });

  it('returns null at the exact edges of the view (inclusive)', () => {
    expect(followTarget(view, 10_000, 0.1, TOTAL, MIN)).toBeNull();
    expect(followTarget(view, 20_000, 0.1, TOTAL, MIN)).toBeNull();
  });

  it('jumps forward, placing the target a lead-in fraction from the left edge', () => {
    const next = followTarget(view, 25_000, 0.1, TOTAL, MIN);
    expect(next).not.toBeNull();
    expect(next!.duration).toBe(view.duration);
    expect(next!.start).toBeCloseTo(25_000 - view.duration * 0.1, 6);
  });

  it('jumps backward when the target is before the view', () => {
    const next = followTarget(view, 1000, 0.1, TOTAL, MIN);
    expect(next).not.toBeNull();
    expect(next!.start).toBeCloseTo(1000 - view.duration * 0.1, 6);
  });

  it('clamps the resulting window to the session bounds', () => {
    const next = followTarget(view, 0, 0.1, TOTAL, MIN);
    expect(next!.start).toBe(0);
  });
});

describe('overlapsRange', () => {
  it('detects overlapping ranges', () => {
    expect(overlapsRange(0, 100, 50, 150)).toBe(true);
    expect(overlapsRange(50, 150, 0, 100)).toBe(true);
  });

  it('detects ranges fully contained within each other', () => {
    expect(overlapsRange(0, 1000, 200, 300)).toBe(true);
    expect(overlapsRange(200, 300, 0, 1000)).toBe(true);
  });

  it('treats touching edges as overlapping', () => {
    expect(overlapsRange(0, 100, 100, 200)).toBe(true);
  });

  it('detects non-overlapping ranges', () => {
    expect(overlapsRange(0, 100, 200, 300)).toBe(false);
    expect(overlapsRange(200, 300, 0, 100)).toBe(false);
  });
});

describe('msToFrac / fracToMs', () => {
  const view: ViewWindow = { start: 1000, duration: 4000 };

  it('round-trip ms -> frac -> ms', () => {
    expect(msToFrac(1000, view)).toBe(0);
    expect(msToFrac(5000, view)).toBe(1);
    expect(msToFrac(3000, view)).toBe(0.5);
    expect(fracToMs(0, view)).toBe(1000);
    expect(fracToMs(1, view)).toBe(5000);
    expect(fracToMs(0.5, view)).toBe(3000);
  });

  it('handles fractions outside [0, 1] (off-screen positions)', () => {
    expect(msToFrac(0, view)).toBe(-0.25);
    expect(fracToMs(1.5, view)).toBe(7000);
  });

  it('does not divide by zero for an empty-duration view', () => {
    expect(() => msToFrac(1000, { start: 1000, duration: 0 })).not.toThrow();
    expect(Number.isFinite(msToFrac(1000, { start: 1000, duration: 0 }))).toBe(true);
  });
});

describe('hitTestOverview', () => {
  // Viewport spans [10_000, 30_000) of a 100_000ms session -> [0.1, 0.3] in fraction space
  const view: ViewWindow = { start: 10_000, duration: 20_000 };
  const tolerance = 0.01;

  it('detects a hit on the left edge', () => {
    expect(hitTestOverview(0.1, view, TOTAL, tolerance)).toBe('resize-left');
    expect(hitTestOverview(0.105, view, TOTAL, tolerance)).toBe('resize-left');
  });

  it('detects a hit on the right edge', () => {
    expect(hitTestOverview(0.3, view, TOTAL, tolerance)).toBe('resize-right');
    expect(hitTestOverview(0.295, view, TOTAL, tolerance)).toBe('resize-right');
  });

  it('detects a hit inside the viewport body', () => {
    expect(hitTestOverview(0.2, view, TOTAL, tolerance)).toBe('move');
  });

  it('detects a hit outside the viewport', () => {
    expect(hitTestOverview(0.05, view, TOTAL, tolerance)).toBe('outside');
    expect(hitTestOverview(0.5, view, TOTAL, tolerance)).toBe('outside');
  });

  it('prefers edge resize over move/outside when both are within tolerance', () => {
    // A very narrow viewport where both edges fall within tolerance of the click.
    const narrow: ViewWindow = { start: 50_000, duration: 100 };
    const result = hitTestOverview(0.5, narrow, TOTAL, 0.01);
    expect(['resize-left', 'resize-right']).toContain(result);
  });
});

describe('sliceVisiblePoints', () => {
  const pts = [0, 1000, 2000, 3000, 4000, 5000].map((ms) => ({ ms }));

  it('returns all points when the view covers the full range', () => {
    expect(sliceVisiblePoints(pts, 0, 5000)).toEqual(pts);
  });

  it('returns an empty array for an empty input', () => {
    expect(sliceVisiblePoints([], 0, 5000)).toEqual([]);
  });

  it('includes one point before the view start for step-graph continuity', () => {
    const result = sliceVisiblePoints(pts, 1500, 3500);
    expect(result[0]).toEqual({ ms: 1000 });
    expect(result[result.length - 1]).toEqual({ ms: 4000 });
  });

  it('clamps to index 0 when the view starts before all points', () => {
    const result = sliceVisiblePoints(pts, -500, 500);
    expect(result[0]).toEqual({ ms: 0 });
  });

  it('includes one point after the view end for step-graph continuity', () => {
    const result = sliceVisiblePoints(pts, 1000, 2000);
    const lastMs = result[result.length - 1].ms;
    expect(lastMs).toBeGreaterThanOrEqual(2000);
  });

  it('only returns the last point when the view is entirely past all points', () => {
    const result = sliceVisiblePoints(pts, 9000, 10_000);
    expect(result).toEqual([{ ms: 5000 }]);
  });
});

describe('chooseTickInterval', () => {
  it('never returns an interval that crowds labels closer than ~60px apart', () => {
    const availPx = 900;
    for (const viewDur of [500, 2000, 30_000, 600_000, 3_600_000]) {
      const interval = chooseTickInterval(viewDur, availPx);
      const gapPx = (interval / viewDur) * availPx;
      expect(gapPx).toBeGreaterThanOrEqual(60 - 1e-9);
    }
  });

  it('falls back to the largest candidate for extremely long views', () => {
    expect(chooseTickInterval(10 * 60 * 60 * 1000, 100)).toBe(600_000);
  });
});
