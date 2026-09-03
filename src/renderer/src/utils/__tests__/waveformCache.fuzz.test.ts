import { describe, it, expect } from 'vitest';
import {
  overscanRange,
  cacheSource,
  densePointRange,
  bitmapRange,
  bitmapIsStale,
  bitmapSharpness,
  type BuiltBitmap,
  type PeakCache
} from '../waveformCache';

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

const ZOOM_LEVELS_SEC = [0.25, 0.5, 1, 2, 5, 10, 20, 30, 60, 120, 300];
const TRACK_DURATION = 360;
const DENSE_RATE = 250;
const OVERSCAN = 1;
const MAX_BITMAP_PX = 8192;
const CANVAS_DEVICE_PX = 2800;
const CANVAS_HEIGHT = 300;

type View = { startSec: number; endSec: number };
type Drawn = { bitmap: BuiltBitmap; spanSec: number } | null;

// createImageBitmap is async, so a frame draws whatever the last build left behind and the
// rebuild lands after it. That lag is the whole point of the model.
class EditViewModel {
  cache: PeakCache | null = null;
  bitmap: BuiltBitmap | null = null;

  frame(view: View): Drawn {
    this.updateCache(view);
    const drawn: Drawn = this.bitmap
      ? { bitmap: this.bitmap, spanSec: this.bitmap.endSec - this.bitmap.startSec }
      : null;
    if (
      this.cache &&
      bitmapIsStale(
        this.bitmap,
        this.cache,
        view.startSec,
        view.endSec,
        CANVAS_HEIGHT,
        MAX_BITMAP_PX
      )
    )
      this.rebuild(view);
    return drawn;
  }

  private updateCache(view: View): void {
    const required = CANVAS_DEVICE_PX / (view.endSec - view.startSec);
    const source = cacheSource(this.cache, view.startSec, view.endSec, required, DENSE_RATE);
    if (source === 'keep') return;
    const { startSec, endSec } = overscanRange(
      view.startSec,
      view.endSec,
      TRACK_DURATION,
      OVERSCAN
    );
    if (source === 'dense') {
      const range = densePointRange(startSec, endSec, DENSE_RATE, TRACK_DURATION * DENSE_RATE);
      if (!range) return;
      this.cache = {
        startSec: range.startIndex / DENSE_RATE,
        endSec: range.endIndex / DENSE_RATE,
        ptsPerSec: DENSE_RATE
      };
      return;
    }
    if (endSec > startSec) this.cache = { startSec, endSec, ptsPerSec: required };
  }

  private rebuild(view: View): void {
    if (!this.cache) return;
    const range = bitmapRange(this.cache, view.startSec, view.endSec, MAX_BITMAP_PX);
    if (!range) return;
    this.bitmap = {
      startSec: range.startSec,
      endSec: range.endSec,
      width: range.width,
      canvasHeight: CANVAS_HEIGHT
    };
  }
}

function randomView(random: () => number): View {
  const span = ZOOM_LEVELS_SEC[Math.floor(random() * ZOOM_LEVELS_SEC.length)];
  const startSec = Math.max(0, Math.min(TRACK_DURATION - span, random() * TRACK_DURATION));
  return { startSec, endSec: startSec + span };
}

describe('the edit waveform under fuzzed zoom and pan', () => {
  it('draws the bitmap across its own span, never the span the cache moved to', () => {
    const random = makeRandom(29);
    const model = new EditViewModel();
    for (let step = 0; step < 3000; step++) {
      const drawn = model.frame(randomView(random));
      if (!drawn) continue;
      const bitmapSpan = drawn.bitmap.endSec - drawn.bitmap.startSec;
      expect(drawn.spanSec / bitmapSpan).toBeCloseTo(1, 10);
    }
  });

  it('holds one source column per device pixel wherever the cache was fetched for the view', () => {
    const random = makeRandom(29);
    const model = new EditViewModel();
    for (let step = 0; step < 3000; step++) {
      const view = randomView(random);
      model.frame(view);
      model.frame(view);
      if (!model.bitmap) continue;
      const devicePxPerSec = CANVAS_DEVICE_PX / (view.endSec - view.startSec);
      expect(bitmapSharpness(model.bitmap, devicePxPerSec)).toBeGreaterThanOrEqual(0.9);
    }
  });

  it('always ends with a cache that spans the view', () => {
    const random = makeRandom(31);
    const model = new EditViewModel();
    for (let step = 0; step < 3000; step++) {
      const view = randomView(random);
      model.frame(view);
      expect(model.cache).not.toBeNull();
      if (!model.cache) continue;
      expect(model.cache.startSec).toBeLessThanOrEqual(view.startSec + 1e-6);
      expect(model.cache.endSec).toBeGreaterThanOrEqual(
        Math.min(view.endSec, TRACK_DURATION) - 1e-6
      );
    }
  });

  it('keeps the bitmap at the cache resolution and covering the view', () => {
    const random = makeRandom(37);
    const model = new EditViewModel();
    for (let step = 0; step < 3000; step++) {
      const view = randomView(random);
      model.frame(view);
      model.frame(view);
      if (!model.cache || !model.bitmap) continue;
      expect(model.bitmap.startSec).toBeLessThanOrEqual(
        Math.max(view.startSec, model.cache.startSec) + 1e-6
      );
      expect(model.bitmap.endSec).toBeGreaterThanOrEqual(
        Math.min(view.endSec, model.cache.endSec) - 1e-6
      );
    }
  });

  it('leaves the bitmap alone when a zoom does not move the cache', () => {
    const random = makeRandom(41);
    const model = new EditViewModel();
    let rebuilds = 0;
    let previous: BuiltBitmap | null = null;
    for (let step = 0; step < 2000; step++) {
      model.frame(randomView(random));
      if (model.bitmap !== previous) rebuilds++;
      previous = model.bitmap;
    }
    expect(rebuilds).toBeLessThan(2000);
  });
});
