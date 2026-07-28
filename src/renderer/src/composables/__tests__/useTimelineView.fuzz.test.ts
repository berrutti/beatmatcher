import { describe, it, expect } from 'vitest';
import { ref, nextTick } from 'vue';
import { useTimelineView } from '@renderer/composables/useTimelineView';
import { LABEL_W, PADDING } from '@renderer/utils/timelineDraw';

const MIN_VIEW_MS = 200;

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

// Well past every range, so the clamps are under test rather than the caller.
function wildValue(random: () => number): number {
  const magnitude = [1, 1e3, 1e6, 1e9][Math.floor(random() * 4)];
  return (random() * 2 - 1) * magnitude;
}

function makeCamera(totalMs: number) {
  return useTimelineView(
    () => totalMs,
    () => 'classic-3band'
  );
}

function expectSaneView(
  camera: ReturnType<typeof makeCamera>,
  totalMs: number,
  label: string
): void {
  const start = camera.viewStartMs.value;
  const duration = camera.viewDurationMs.value;
  const total = Math.max(totalMs, MIN_VIEW_MS);
  expect(Number.isFinite(start), label).toBe(true);
  expect(Number.isFinite(duration), label).toBe(true);
  expect(start, label).toBeGreaterThanOrEqual(0);
  expect(duration, label).toBeGreaterThanOrEqual(MIN_VIEW_MS);
  expect(duration, label).toBeLessThanOrEqual(total);
  expect(start + duration, label).toBeLessThanOrEqual(total + 1e-6);
}

describe('timeline camera under fuzzed input', () => {
  it('keeps the view inside the session however it is zoomed and panned', () => {
    const totalMs = 240_000;
    const camera = makeCamera(totalMs);
    const random = makeRandom(3);
    let moves = 0;

    for (let step = 0; step < 3000; step++) {
      const before = camera.viewStartMs.value + camera.viewDurationMs.value;
      switch (Math.floor(random() * 4)) {
        case 0:
          camera.zoomAt(random(), wildValue(random));
          break;
        case 1:
          camera.panByPixels(wildValue(random), random() * 2000);
          break;
        case 2:
          camera.panByMsDelta(wildValue(random));
          break;
        default:
          camera.setViewFromUser({ start: wildValue(random), duration: wildValue(random) });
      }
      expectSaneView(camera, totalMs, `step ${step}`);
      if (camera.viewStartMs.value + camera.viewDurationMs.value !== before) moves++;
    }

    expect(moves).toBeGreaterThan(100);
  });

  it('refuses to let a non-finite input reach the view', () => {
    const totalMs = 120_000;
    const random = makeRandom(41);
    const wild = [NaN, Infinity, -Infinity];
    const pick = () => (random() < 0.5 ? wild[Math.floor(random() * 3)] : wildValue(random));

    for (let step = 0; step < 2000; step++) {
      const camera = makeCamera(totalMs);
      camera.setViewFromUser({ start: 40_000, duration: 10_000 });
      switch (Math.floor(random() * 5)) {
        case 0:
          camera.zoomAt(pick(), pick());
          break;
        case 1:
          camera.panByPixels(pick(), pick());
          break;
        case 2:
          camera.panByMsDelta(pick());
          break;
        case 3:
          camera.followPlayhead(pick());
          break;
        default:
          camera.setViewFromUser({ start: pick(), duration: pick() });
      }
      expectSaneView(camera, totalMs, `step ${step}`);
    }
  });

  it('ignores a non-finite playhead rather than jumping the view to zero', () => {
    const camera = makeCamera(120_000);
    camera.setViewFromUser({ start: 40_000, duration: 10_000 });
    camera.followPlayhead(NaN);
    expect(camera.viewStartMs.value).toBe(40_000);
    expect(camera.viewDurationMs.value).toBe(10_000);
  });

  it('survives a zero-width track, which is what a hidden canvas reports', () => {
    const totalMs = 60_000;
    const camera = makeCamera(totalMs);
    const random = makeRandom(5);

    for (let step = 0; step < 500; step++) {
      camera.panByPixels(wildValue(random), 0);
      expectSaneView(camera, totalMs, `step ${step}`);
    }
  });

  it('lands the playhead inside the view whenever follow is armed', () => {
    const totalMs = 300_000;
    const camera = makeCamera(totalMs);
    const random = makeRandom(13);
    let reframes = 0;

    for (let step = 0; step < 3000; step++) {
      camera.zoomAt(random(), wildValue(random));
      const ms = random() * totalMs;
      const before = camera.viewStartMs.value;
      camera.followPlayhead(ms);
      if (camera.viewStartMs.value !== before) reframes++;

      const start = camera.viewStartMs.value;
      const end = start + camera.viewDurationMs.value;
      expect(ms >= start && ms <= end, `step ${step}`).toBe(true);
      expectSaneView(camera, totalMs, `step ${step}`);
    }

    expect(reframes).toBeGreaterThan(100);
  });

  it('never lets follow overwrite a view the user just set', () => {
    const totalMs = 300_000;
    const camera = makeCamera(totalMs);
    const random = makeRandom(17);
    let held = 0;

    for (let step = 0; step < 2000; step++) {
      camera.setViewFromUser({ start: random() * totalMs, duration: 1_000 + random() * 20_000 });
      const start = camera.viewStartMs.value;
      const duration = camera.viewDurationMs.value;

      for (let tick = 0; tick < 20; tick++) {
        const ms = random() * totalMs;
        camera.followPlayhead(ms);
        if (ms >= start && ms <= start + duration) break;
        expect(camera.viewStartMs.value, `step ${step} tick ${tick}`).toBe(start);
        expect(camera.viewDurationMs.value, `step ${step} tick ${tick}`).toBe(duration);
        held++;
      }
    }

    expect(held).toBeGreaterThan(100);
  });

  it('keeps vertical scroll inside the content however it is resized', () => {
    const camera = makeCamera(60_000);
    const random = makeRandom(23);

    for (let step = 0; step < 2000; step++) {
      camera.scrollY.value = wildValue(random);
      camera.setContentMetrics(Math.abs(wildValue(random)), Math.abs(wildValue(random)));

      expect(Number.isFinite(camera.scrollY.value), `step ${step}`).toBe(true);
      expect(camera.scrollY.value, `step ${step}`).toBeGreaterThanOrEqual(0);
      expect(camera.scrollY.value, `step ${step}`).toBeLessThanOrEqual(camera.maxScrollY());
    }
  });

  it('projects ms to x and back without drifting, at any canvas size', () => {
    const totalMs = 180_000;
    const camera = makeCamera(totalMs);
    const random = makeRandom(29);

    for (let step = 0; step < 2000; step++) {
      camera.setViewFromUser({ start: wildValue(random), duration: wildValue(random) });
      const canvasW = LABEL_W + PADDING + random() * 2000;
      const viewContext = camera.viewContext(canvasW, 100 + random() * 900);
      const view = camera.currentView();
      const ms = view.start + random() * view.duration;
      const x = viewContext.msToX(ms);

      expect(Number.isFinite(x), `step ${step}`).toBe(true);
      if (viewContext.trackW > 1) {
        expect(viewContext.xToMs(x), `step ${step}`).toBeCloseTo(ms, 3);
      }
    }
  });

  it('reframes to the whole session when the duration changes under it', async () => {
    const totalMs = ref(120_000);
    const camera = useTimelineView(
      () => totalMs.value,
      () => 'classic-3band'
    );
    const random = makeRandom(31);

    for (let step = 0; step < 200; step++) {
      camera.setViewFromUser({ start: wildValue(random), duration: wildValue(random) });
      totalMs.value = 1_000 + Math.floor(random() * 500_000);
      await nextTick();

      expect(camera.viewStartMs.value, `step ${step}`).toBe(0);
      expect(camera.viewDurationMs.value, `step ${step}`).toBe(totalMs.value);
      expectSaneView(camera, totalMs.value, `step ${step}`);
    }
  });
});
