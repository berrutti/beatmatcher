import { describe, it, expect } from 'vitest';
import {
  overscanRange,
  cacheCoversView,
  densePointRange,
  bitmapRange,
  cacheSource,
  bitmapIsStale
} from '../waveformCache';

describe('overscanRange', () => {
  it('pads the view by its own span on each side', () => {
    expect(overscanRange(10, 20, 600, 1)).toEqual({ startSec: 0, endSec: 30 });
  });

  it('clamps to the track rather than reaching past either end', () => {
    expect(overscanRange(2, 12, 15, 1)).toEqual({ startSec: 0, endSec: 15 });
  });

  it('returns the view itself when there is no overscan', () => {
    expect(overscanRange(10, 20, 600, 0)).toEqual({ startSec: 10, endSec: 20 });
  });
});

describe('cacheCoversView', () => {
  const cache = { startSec: 10, endSec: 30, ptsPerSec: 250 };

  it('accepts a cache that spans the view at sufficient density', () => {
    expect(cacheCoversView(cache, 15, 25, 250)).toBe(true);
  });

  it('rejects a view that starts before the cache', () => {
    expect(cacheCoversView(cache, 5, 25, 250)).toBe(false);
  });

  it('rejects a view that ends after the cache', () => {
    expect(cacheCoversView(cache, 15, 35, 250)).toBe(false);
  });

  it('rejects a cache too sparse for the zoom', () => {
    expect(cacheCoversView(cache, 15, 25, 1000)).toBe(false);
  });

  it('accepts a cache a tenth under the required density', () => {
    expect(cacheCoversView(cache, 15, 25, 270)).toBe(true);
    expect(cacheCoversView(cache, 15, 25, 280)).toBe(false);
  });

  it('accepts a view sitting exactly on the cache edges', () => {
    expect(cacheCoversView(cache, 10, 30, 250)).toBe(true);
  });
});

describe('densePointRange', () => {
  it('covers the requested seconds, rounding outwards', () => {
    expect(densePointRange(1.5, 2.5, 100, 1000)).toEqual({ startIndex: 150, endIndex: 250 });
  });

  it('never runs past the points held', () => {
    expect(densePointRange(0, 100, 100, 500)).toEqual({ startIndex: 0, endIndex: 500 });
  });

  it('returns nothing when the range holds no point', () => {
    expect(densePointRange(9, 9, 100, 500)).toBeNull();
    expect(densePointRange(10, 20, 100, 500)).toBeNull();
  });
});

describe('bitmapRange', () => {
  const cache = { startSec: 10, endSec: 20, ptsPerSec: 250 };

  it('covers the whole cache when its points fit', () => {
    expect(bitmapRange(cache, 12, 14, 8192)).toEqual({ startSec: 10, endSec: 20, width: 2500 });
  });

  it('shrinks the span around the view rather than thinning the columns', () => {
    const wide = { startSec: 0, endSec: 338, ptsPerSec: 250 };
    const range = bitmapRange(wide, 100, 101, 8192);
    if (!range) throw new Error('expected a range');
    expect(range.width).toBe(8192);
    expect(range.width / (range.endSec - range.startSec)).toBeCloseTo(250, 6);
    expect(range.startSec).toBeLessThanOrEqual(100);
    expect(range.endSec).toBeGreaterThanOrEqual(101);
  });

  it('stays inside the cache at either end', () => {
    const wide = { startSec: 0, endSec: 338, ptsPerSec: 250 };
    expect(bitmapRange(wide, 0, 1, 8192)?.startSec).toBe(0);
    expect(bitmapRange(wide, 337, 338, 8192)?.endSec).toBeCloseTo(338, 6);
  });

  it('returns nothing for a cache with no span or no rate', () => {
    expect(bitmapRange({ startSec: 5, endSec: 5, ptsPerSec: 250 }, 5, 5, 8192)).toBeNull();
    expect(bitmapRange({ startSec: 0, endSec: 10, ptsPerSec: 0 }, 0, 10, 8192)).toBeNull();
  });
});

describe('cacheSource', () => {
  const dense = { startSec: 0, endSec: 60, ptsPerSec: 250 };

  it('keeps a dense cache that already covers the view', () => {
    expect(cacheSource(dense, 10, 20, 100, 250)).toBe('keep');
  });

  it('reaches for dense when it is dense enough for the zoom', () => {
    expect(cacheSource(null, 10, 20, 100, 250)).toBe('dense');
  });

  it('fetches when the zoom needs more than dense holds', () => {
    expect(cacheSource(null, 10, 10.5, 5000, 250)).toBe('fetch');
  });

  it('keeps a fetched cache that still covers a deep zoom', () => {
    const fetched = { startSec: 10, endSec: 11, ptsPerSec: 5000 };
    expect(cacheSource(fetched, 10.2, 10.7, 5000, 250)).toBe('keep');
  });

  it('fetches once a deep zoom pans off the fetched range', () => {
    const fetched = { startSec: 10, endSec: 11, ptsPerSec: 5000 };
    expect(cacheSource(fetched, 12, 12.5, 5000, 250)).toBe('fetch');
  });
});

describe('bitmapIsStale', () => {
  const cache = { startSec: 10, endSec: 20, ptsPerSec: 250 };
  const built = { startSec: 10, endSec: 20, width: 2500, canvasHeight: 300 };

  it('is fresh while it covers the view at the cache resolution', () => {
    expect(bitmapIsStale(built, cache, 12, 14, 300, 8192)).toBe(false);
  });

  it('goes stale once the view leaves the built window', () => {
    const windowed = { ...built, startSec: 10, endSec: 13 };
    expect(bitmapIsStale(windowed, cache, 14, 16, 300, 8192)).toBe(true);
  });

  it('goes stale when the cache changes resolution under it', () => {
    expect(bitmapIsStale(built, { ...cache, ptsPerSec: 5000 }, 12, 14, 300, 8192)).toBe(true);
  });

  it('goes stale when the canvas changes height', () => {
    expect(bitmapIsStale(built, cache, 12, 14, 400, 8192)).toBe(true);
  });

  it('is stale when nothing has been built', () => {
    expect(bitmapIsStale(null, cache, 12, 14, 300, 8192)).toBe(true);
  });
});
