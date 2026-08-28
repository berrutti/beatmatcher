import { describe, it, expect } from 'vitest';
import { drawDeckLanes, LABEL_W, type SublaneLayout } from '@renderer/utils/timelineDraw';
import type { DeckLanes, LanePoint } from '@renderer/utils/types';

type Stroke = { alpha: number; color: string; points: [number, number][] };

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

const CANVAS_W = 900;
const VIEW_MS = 60_000;
const LANE: SublaneLayout = { key: 'filter', top: 100, height: 90 };

function deckLanes(filter: LanePoint[]): DeckLanes {
  return {
    gain: [],
    eqLow: [],
    eqMid: [],
    eqHigh: [],
    filter,
    rate: [],
    rateMin: 0.9,
    rateMax: 1.1,
    filterActive: []
  };
}

function strokesFor(points: LanePoint[], viewStart: number, viewEnd: number): Stroke[] {
  const strokes: Stroke[] = [];
  let alpha = 1;
  let color = '';
  let current: [number, number][] = [];
  const stack: number[] = [];
  const ctx = {
    beginPath: () => {
      current = [];
    },
    moveTo: (x: number, y: number) => current.push([x, y]),
    lineTo: (x: number, y: number) => current.push([x, y]),
    arcTo: (_cornerX: number, _cornerY: number, x: number, y: number) => current.push([x, y]),
    stroke: () => strokes.push({ alpha, color, points: [...current] }),
    fillRect: () => {},
    fillText: () => {},
    rect: () => {},
    clip: () => {},
    setLineDash: () => {},
    save: () => stack.push(alpha),
    restore: () => {
      alpha = stack.pop() ?? 1;
    },
    set globalAlpha(value: number) {
      alpha = value;
    },
    get globalAlpha() {
      return alpha;
    },
    set strokeStyle(value: string) {
      color = value;
    },
    get strokeStyle() {
      return color;
    },
    set fillStyle(_value: string) {},
    set lineWidth(_value: number) {},
    set lineJoin(_value: string) {},
    set lineCap(_value: string) {},
    set font(_value: string) {},
    set textAlign(_value: string) {},
    set textBaseline(_value: string) {}
  } as unknown as CanvasRenderingContext2D;

  drawDeckLanes(
    ctx,
    CANVAS_W,
    (ms: number) => LABEL_W + ((ms - viewStart) / (viewEnd - viewStart)) * (CANVAS_W - LABEL_W),
    deckLanes(points),
    [LANE],
    viewStart,
    viewEnd,
    'classic-3band',
    [],
    new Map(),
    '#ffffff'
  );
  // The dashed centre line is drawn across the whole track, values are not.
  return strokes.filter((stroke) => stroke.points.length > 0 && stroke.color !== '#4a4a4a');
}

function fuzzedCurve(random: () => number): LanePoint[] {
  const points: LanePoint[] = [];
  let ms = random() * 2000;
  let value = (random() - 0.5) * 2;
  const count = 2 + Math.floor(random() * 200);
  for (let idx = 0; idx < count; idx++) {
    points.push({ ms, value });
    ms += random() * 900;
    // Half the time nudge the value, so runs at the default and on its edge are common.
    value =
      random() < 0.5
        ? (random() - 0.5) * 0.12
        : Math.max(-1, Math.min(1, value + (random() - 0.5)));
  }
  return points;
}

describe('the lane curve under fuzzed value sequences', () => {
  it('draws one colour at one of the two strengths, whatever the values do', () => {
    const random = makeRandom(29);
    for (let step = 0; step < 400; step++) {
      const strokes = strokesFor(fuzzedCurve(random), 0, VIEW_MS);
      expect(new Set(strokes.map((stroke) => stroke.color)).size).toBeLessThanOrEqual(1);
      for (const stroke of strokes) expect([0.3, 1]).toContain(stroke.alpha);
    }
  });

  it('never leaves the lane band, so no curve bleeds into its neighbours', () => {
    const random = makeRandom(31);
    for (let step = 0; step < 400; step++) {
      for (const stroke of strokesFor(fuzzedCurve(random), 0, VIEW_MS)) {
        for (const [x, y] of stroke.points) {
          expect(Number.isFinite(x)).toBe(true);
          expect(y).toBeGreaterThanOrEqual(LANE.top);
          expect(y).toBeLessThanOrEqual(LANE.top + LANE.height);
        }
      }
    }
  });

  it('draws an unbroken curve: every piece starts where the last one ended', () => {
    const random = makeRandom(37);
    for (let step = 0; step < 400; step++) {
      const strokes = strokesFor(fuzzedCurve(random), 0, VIEW_MS);
      for (let idx = 1; idx < strokes.length; idx++) {
        const previousEnd = strokes[idx - 1].points[strokes[idx - 1].points.length - 1];
        expect(strokes[idx].points[0]).toEqual(previousEnd);
      }
    }
  });

  it('survives a scrolled or zoomed view without drawing anything absurd', () => {
    const random = makeRandom(41);
    for (let step = 0; step < 300; step++) {
      const viewStart = random() * 50_000;
      const viewEnd = viewStart + 200 + random() * 90_000;
      const strokes = strokesFor(fuzzedCurve(random), viewStart, viewEnd);
      for (const stroke of strokes) {
        for (const [x, y] of stroke.points) {
          expect(Number.isNaN(x)).toBe(false);
          expect(Number.isNaN(y)).toBe(false);
        }
      }
    }
  });
});
