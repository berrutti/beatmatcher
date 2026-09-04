import { describe, it, expect } from 'vitest';
import { anchoredView, clampedView, visibleSpanLabel } from '../waveformZoom';

const TRACK = 360;

describe('clampedView', () => {
  it('keeps a window that fits where it was asked for', () => {
    expect(clampedView(50, 20, TRACK)).toEqual({ startSec: 50, endSec: 70 });
  });

  it('pulls a window back inside the track rather than past its end', () => {
    expect(clampedView(355, 20, TRACK)).toEqual({ startSec: 340, endSec: 360 });
    expect(clampedView(-10, 20, TRACK)).toEqual({ startSec: 0, endSec: 20 });
  });

  it('shows the whole track when asked for more than it holds', () => {
    expect(clampedView(100, 900, TRACK)).toEqual({ startSec: 0, endSec: 360 });
  });
});

describe('anchoredView', () => {
  it('leaves the anchored second at the same fraction across the width', () => {
    for (const frac of [0, 0.25, 0.5, 0.9, 1]) {
      for (const duration of [5, 20, 120]) {
        const view = anchoredView(180, frac, duration, TRACK);
        const at = view.startSec + frac * (view.endSec - view.startSec);
        expect(at).toBeCloseTo(180, 6);
      }
    }
  });

  it('holds the anchor through a whole zoom ladder without drifting', () => {
    let view = { startSec: 40, endSec: 60 };
    const frac = 0.3;
    const anchor = view.startSec + frac * (view.endSec - view.startSec);
    for (const duration of [10, 5, 2, 5, 10, 20]) {
      view = anchoredView(anchor, frac, duration, TRACK);
      expect(view.startSec + frac * (view.endSec - view.startSec)).toBeCloseTo(anchor, 6);
    }
  });

  it('gives up the anchor only where the track ends', () => {
    const view = anchoredView(2, 0.9, 20, TRACK);
    expect(view).toEqual({ startSec: 0, endSec: 20 });
  });
});

describe('visibleSpanLabel', () => {
  it('names a whole ladder level without a decimal', () => {
    expect(visibleSpanLabel(300, TRACK)).toBe('5m');
    expect(visibleSpanLabel(120, TRACK)).toBe('2m');
    expect(visibleSpanLabel(20, TRACK)).toBe('20s');
  });

  it('names the track, not the level, once the level is wider than the track', () => {
    expect(visibleSpanLabel(300, 240)).toBe('4m');
    expect(visibleSpanLabel(300, 227.4)).toBe('3.8m');
    expect(visibleSpanLabel(60, 47.3)).toBe('47s');
  });

  it('falls back to the level while no track is loaded', () => {
    expect(visibleSpanLabel(300, 0)).toBe('5m');
  });

  it('keeps the sub-second levels apart instead of rounding them all to zero', () => {
    expect(visibleSpanLabel(0.25, TRACK)).toBe('0.25s');
    expect(visibleSpanLabel(0.5, TRACK)).toBe('0.5s');
    expect(visibleSpanLabel(1, TRACK)).toBe('1s');
  });
});
