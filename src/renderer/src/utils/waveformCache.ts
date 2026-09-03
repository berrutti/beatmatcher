export type PeakCache = { startSec: number; endSec: number; ptsPerSec: number };

export type PointRange = { startIndex: number; endIndex: number };

// The view span again on each side, so a pan of up to one screen needs no refetch.
export function overscanRange(
  viewStartSec: number,
  viewEndSec: number,
  trackDuration: number,
  overscanFactor: number
): { startSec: number; endSec: number } {
  const pad = (viewEndSec - viewStartSec) * overscanFactor;
  return {
    startSec: Math.max(0, viewStartSec - pad),
    endSec: Math.min(trackDuration, viewEndSec + pad)
  };
}

// A tenth under the required density still reads, and refusing it refetches on every zoom.
export function cacheCoversView(
  cache: PeakCache,
  viewStartSec: number,
  viewEndSec: number,
  requiredPtsPerSec: number
): boolean {
  return (
    cache.startSec <= viewStartSec + 1e-6 &&
    cache.endSec >= viewEndSec - 1e-6 &&
    cache.ptsPerSec >= requiredPtsPerSec * 0.9
  );
}

export function densePointRange(
  startSec: number,
  endSec: number,
  denseRate: number,
  totalPoints: number
): PointRange | null {
  const startIndex = Math.max(0, Math.floor(startSec * denseRate));
  const endIndex = Math.min(totalPoints, Math.ceil(endSec * denseRate));
  return endIndex > startIndex ? { startIndex, endIndex } : null;
}

export type BitmapRange = { startSec: number; endSec: number; width: number };

// One column per cached point wherever that fits: the span shrinks around the view before
// the resolution ever thins, or a cache far wider than the view would leave a deep zoom a
// handful of columns to stretch. The visible span is always covered.
export function bitmapRange(
  cache: PeakCache,
  viewStartSec: number,
  viewEndSec: number,
  maxWidth: number
): BitmapRange | null {
  const cacheSpan = cache.endSec - cache.startSec;
  if (cacheSpan <= 0 || cache.ptsPerSec <= 0 || maxWidth < 1) return null;

  if (Math.round(cacheSpan * cache.ptsPerSec) <= maxWidth) {
    const width = Math.round(cacheSpan * cache.ptsPerSec);
    return width >= 1 ? { startSec: cache.startSec, endSec: cache.endSec, width } : null;
  }

  const visibleStart = Math.max(cache.startSec, viewStartSec);
  const visibleEnd = Math.min(cache.endSec, viewEndSec);
  const windowSpan = Math.min(
    cacheSpan,
    Math.max(maxWidth / cache.ptsPerSec, visibleEnd - visibleStart)
  );
  const centre = (visibleStart + visibleEnd) / 2;
  const startSec = Math.min(
    Math.max(centre - windowSpan / 2, cache.startSec),
    cache.endSec - windowSpan
  );
  const width = Math.min(Math.round(windowSpan * cache.ptsPerSec), maxWidth);
  return width >= 1 ? { startSec, endSec: startSec + windowSpan, width } : null;
}

export type CacheSource = 'keep' | 'dense' | 'fetch';

export function cacheSource(
  cache: PeakCache | null,
  viewStartSec: number,
  viewEndSec: number,
  requiredPtsPerSec: number,
  denseRate: number
): CacheSource {
  const covers =
    cache !== null && cacheCoversView(cache, viewStartSec, viewEndSec, requiredPtsPerSec);
  if (covers && cache.ptsPerSec === denseRate) return 'keep';
  if (denseRate > 0 && denseRate >= requiredPtsPerSec * 0.9) return 'dense';
  return covers ? 'keep' : 'fetch';
}

export type BuiltBitmap = {
  startSec: number;
  endSec: number;
  width: number;
  canvasHeight: number;
};

function columnsPerSec(range: BitmapRange): number {
  const span = range.endSec - range.startSec;
  return span > 0 ? range.width / span : 0;
}

// Compared against the range that would be built now, because a capped bitmap holds fewer
// columns per second than the cache does and the difference is exactly what goes blurry.
export function bitmapIsStale(
  bitmap: BuiltBitmap | null,
  cache: PeakCache,
  viewStartSec: number,
  viewEndSec: number,
  canvasHeight: number,
  maxWidth: number
): boolean {
  if (!bitmap) return true;
  if (bitmap.canvasHeight !== canvasHeight) return true;

  const wanted = bitmapRange(cache, viewStartSec, viewEndSec, maxWidth);
  if (!wanted) return false;
  if (columnsPerSec(bitmap) < columnsPerSec(wanted) * 0.9) return true;

  return (
    bitmap.startSec > Math.max(cache.startSec, viewStartSec) + 1e-6 ||
    bitmap.endSec < Math.min(cache.endSec, viewEndSec) - 1e-6
  );
}

// Source columns per device pixel where the bitmap is drawn. Below 1 the picture is being
// stretched past the data it holds.
export function bitmapSharpness(bitmap: BuiltBitmap, devicePxPerSec: number): number {
  const drawnWidth = (bitmap.endSec - bitmap.startSec) * devicePxPerSec;
  return drawnWidth > 0 ? bitmap.width / drawnWidth : Infinity;
}
