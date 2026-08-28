import { describe, it, expect } from 'vitest';
import { drawDeckLanes, LABEL_W, type SublaneLayout } from '@renderer/utils/timelineDraw';
import { laneSpecs } from '@renderer/utils/sessionCore';
import type { DeckLanes, LanePoint } from '@renderer/utils/types';

type Stroke = { alpha: number; color: string; from: [number, number]; to: [number, number] };
type Fill = { color: string; x: number; y: number; w: number; h: number };

const MIXER = 'classic-3band';
const CANVAS_W = 500;
const VIEW_MS = 10_000;
const LANE: SublaneLayout = { key: 'gain', top: 100, height: 80 };
let lanes: SublaneLayout[] = [LANE];

function deckLanes(
  gain: LanePoint[],
  filterActive: DeckLanes['filterActive'] = [],
  filter: LanePoint[] = []
): DeckLanes {
  return {
    gain,
    eqLow: [],
    eqMid: [],
    eqHigh: [],
    filter,
    rate: [],
    rateMin: 0.9,
    rateMax: 1.1,
    filterActive
  };
}

const dashes: number[][] = [];
const fills: Fill[] = [];

function fillsForLanes(sublanes: SublaneLayout[]): Fill[] {
  fills.length = 0;
  const previous = lanes;
  lanes = sublanes;
  strokesFor([]);
  lanes = previous;
  return [...fills];
}

function strokesFor(
  gain: LanePoint[],
  spans: DeckLanes['filterActive'] = [],
  filter: LanePoint[] = [],
  highlight: { lane: 'filter'; startMs: number; endMs: number } | null = null
): Stroke[] {
  const strokes: Stroke[] = [];
  let alpha = 1;
  const stack: number[] = [];
  let color = '';
  let fillColor = '';
  let from: [number, number] = [0, 0];
  let to: [number, number] = [0, 0];
  const ctx = {
    beginPath: () => {},
    moveTo: (x: number, y: number) => {
      from = [x, y];
    },
    lineTo: (x: number, y: number) => {
      to = [x, y];
    },
    arcTo: (_cornerX: number, _cornerY: number, x: number, y: number) => {
      to = [x, y];
    },
    stroke: () => strokes.push({ alpha, color, from, to }),
    fillRect: (x: number, y: number, w: number, h: number) =>
      fills.push({ color: fillColor, x, y, w, h }),
    fillText: () => {},
    rect: () => {},
    clip: () => {},
    setLineDash: (pattern: number[]) => dashes.push(pattern),
    save: () => {
      stack.push(alpha);
    },
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
    set fillStyle(value: string) {
      fillColor = value;
    },
    get fillStyle() {
      return fillColor;
    },
    set lineWidth(_value: number) {},
    set font(_value: string) {},
    set textAlign(_value: string) {},
    set textBaseline(_value: string) {}
  } as unknown as CanvasRenderingContext2D;

  drawDeckLanes(
    ctx,
    CANVAS_W,
    (ms: number) => LABEL_W + (ms / VIEW_MS) * (CANVAS_W - LABEL_W),
    deckLanes(gain, spans, filter),
    lanes,
    0,
    VIEW_MS,
    MIXER,
    [],
    new Map(),
    '#ffffff',
    highlight
  );
  return strokes;
}

const flat = (stroke: Stroke) => stroke.from[1] === stroke.to[1];

describe('a lane parked at its default is drawn tamer', () => {
  const { defaultValue } = laneSpecs(MIXER).gain;

  it('dims a run held at the default', () => {
    const strokes = strokesFor([
      { ms: 0, value: defaultValue },
      { ms: 5000, value: defaultValue }
    ]).filter(flat);

    expect(strokes).not.toHaveLength(0);
    for (const stroke of strokes) expect(stroke.alpha).toBeLessThan(1);
  });

  it('leaves a run that moved off the default at full strength', () => {
    const strokes = strokesFor([
      { ms: 0, value: defaultValue * 0.5 },
      { ms: 5000, value: defaultValue * 0.5 }
    ]).filter(flat);

    expect(strokes).not.toHaveLength(0);
    for (const stroke of strokes) expect(stroke.alpha).toBe(1);
  });

  it('keeps the lane colour, dimming only its strength', () => {
    const [parked] = strokesFor([
      { ms: 0, value: defaultValue },
      { ms: 5000, value: defaultValue }
    ]).filter(flat);
    const [moved] = strokesFor([
      { ms: 0, value: defaultValue * 0.5 },
      { ms: 5000, value: defaultValue * 0.5 }
    ]).filter(flat);

    expect(parked.color).toBe(moved.color);
  });

  it('draws the departure from the default at full strength', () => {
    const strokes = strokesFor([
      { ms: 0, value: defaultValue },
      { ms: 5000, value: defaultValue * 0.5 },
      { ms: 9000, value: defaultValue * 0.5 }
    ]);

    const rise = strokes.filter((stroke) => !flat(stroke));
    expect(rise).not.toHaveLength(0);
    for (const stroke of rise) expect(stroke.alpha).toBe(1);
  });
});

describe('the neutral position is marked with a solid line', () => {
  it('dashes nothing on the rate lane', () => {
    dashes.length = 0;
    lanes = [{ key: 'rate', top: 100, height: 80 }];
    strokesFor([]);
    lanes = [LANE];

    expect(dashes.every((pattern) => pattern.length === 0)).toBe(true);
  });

  it('dashes nothing on the filter lane', () => {
    dashes.length = 0;
    lanes = [{ key: 'filter', top: 100, height: 80 }];
    strokesFor([]);
    lanes = [LANE];

    expect(dashes.every((pattern) => pattern.length === 0)).toBe(true);
  });
});

describe('a lane looks the same whatever is stacked around it', () => {
  const rate: SublaneLayout = { key: 'rate', top: 180, height: 80 };
  const filter: SublaneLayout = { key: 'filter', top: 100, height: 80 };

  function backgroundOf(sublanes: SublaneLayout[], lane: SublaneLayout): Fill | undefined {
    return fillsForLanes(sublanes).find((fill) => fill.y === lane.top && fill.h === lane.height);
  }

  function topBorderOf(sublanes: SublaneLayout[], lane: SublaneLayout): Fill | undefined {
    return fillsForLanes(sublanes).find((fill) => fill.y === lane.top && fill.h === 1);
  }

  it('paints every lane the same background, so nothing zebras by group', () => {
    const filterBg = backgroundOf([filter, rate], filter);
    const rateBg = backgroundOf([filter, rate], rate);

    expect(filterBg?.color).toBe(rateBg?.color);
  });

  it('frames a lane the same whether or not another lane precedes it', () => {
    const stacked = topBorderOf([filter, rate], rate);
    const alone = topBorderOf([{ ...rate, top: 100 }], { ...rate, top: 100 });

    expect(stacked?.color).toBe(alone?.color);
  });
});

describe('an active filter span', () => {
  it('tints the lane once, with no bar of its own', () => {
    const lane: SublaneLayout = { key: 'filter', top: 100, height: 80 };
    fills.length = 0;
    const previous = lanes;
    lanes = [lane];
    strokesFor([], [{ startMs: 1000, endMs: 5000 }]);
    lanes = previous;

    const spanX = LABEL_W + (1000 / VIEW_MS) * (CANVAS_W - LABEL_W);
    expect(fills.filter((fill) => fill.x === spanX)).toHaveLength(1);
  });
});

describe('the filter dead zone reads as off', () => {
  it('dims a run inside it, since the filter is doing nothing there', () => {
    const previous = lanes;
    lanes = [{ key: 'filter', top: 100, height: 80 }];
    const strokes = strokesFor(
      [],
      [],
      [
        { ms: 0, value: 0.001 },
        { ms: 5000, value: 0.001 }
      ]
    );
    lanes = previous;

    const flatValueRuns = strokes.filter(
      (stroke) => stroke.from[1] === stroke.to[1] && stroke.to[0] < CANVAS_W - 20
    );
    expect(flatValueRuns).not.toHaveLength(0);
    for (const stroke of flatValueRuns) expect(stroke.alpha).toBeLessThan(1);
  });
});

describe('a value hovering on the dead-zone edge', () => {
  it('draws no isolated dashes where it pokes out for a pixel or two', () => {
    const wobble: LanePoint[] = [];
    for (let ms = 1000; ms <= 9000; ms += 100) {
      wobble.push({ ms, value: ms === 3000 || ms === 5000 || ms === 7000 ? -0.06 : 0 });
    }
    const previous = lanes;
    lanes = [{ key: 'filter', top: 100, height: 80 }];
    const strokes = strokesFor([], [], wobble);
    lanes = previous;

    const curve = strokes.filter((stroke) => stroke.to[0] < CANVAS_W - 20);
    expect(new Set(curve.map((stroke) => `${stroke.color}:${stroke.alpha}`)).size).toBe(1);
  });
});

describe('a curve passing through the dead zone', () => {
  function filterStrokes(points: LanePoint[]) {
    const previous = lanes;
    lanes = [{ key: 'filter', top: 100, height: 80 }];
    const strokes = strokesFor([], [], points);
    lanes = previous;
    return strokes.filter((stroke) => stroke.to[0] < CANVAS_W - 20);
  }

  it('draws one colour, however far the knob swings either side', () => {
    const curve = filterStrokes([
      { ms: 0, value: -0.6 },
      { ms: 3000, value: 0 },
      { ms: 3200, value: 0.6 },
      { ms: 9000, value: 0.6 }
    ]);

    expect(new Set(curve.map((stroke) => stroke.color)).size).toBe(1);
  });

  it('stays at full strength when it only passes through', () => {
    const curve = filterStrokes([
      { ms: 0, value: -0.6 },
      { ms: 3000, value: 0 },
      { ms: 3200, value: 0.6 },
      { ms: 9000, value: 0.6 }
    ]);

    for (const stroke of curve) expect(stroke.alpha).toBe(1);
  });

  it('dims a long stretch parked in it, which is the filter doing nothing', () => {
    const curve = filterStrokes([
      { ms: 0, value: -0.6 },
      { ms: 2000, value: 0 },
      { ms: 8000, value: 0.6 },
      { ms: 9000, value: 0.6 }
    ]);

    expect(curve.some((stroke) => stroke.alpha < 1)).toBe(true);
  });
});

describe('the span a reset would clear', () => {
  it('is drawn on the curve itself, leaving the rest of it alone', () => {
    const previous = lanes;
    lanes = [{ key: 'filter', top: 100, height: 80 }];
    const points: LanePoint[] = [
      { ms: 0, value: 0.6 },
      { ms: 3000, value: 0.6 },
      { ms: 6000, value: 0.9 },
      { ms: 9000, value: 0.9 }
    ];
    const plain = strokesFor([], [], points).filter((stroke) => stroke.to[0] < CANVAS_W - 20);
    const lit = strokesFor([], [], points, { lane: 'filter', startMs: 0, endMs: 3000 }).filter(
      (stroke) => stroke.to[0] < CANVAS_W - 20
    );
    lanes = previous;

    const colors = new Set(lit.map((stroke) => stroke.color));
    expect(colors.size).toBe(2);
    expect(colors).toContain(plain[0].color);
    for (const stroke of lit) expect(stroke.from[1]).toBeGreaterThan(0);
  });
});

describe('two moves either side of a crossing', () => {
  const points: LanePoint[] = [
    { ms: 0, value: 0.6 },
    { ms: 3000, value: 0.6 },
    { ms: 3100, value: -0.6 },
    { ms: 9000, value: -0.6 }
  ];

  function litRuns(startMs: number, endMs: number) {
    const previous = lanes;
    lanes = [{ key: 'filter', top: 100, height: 80 }];
    const strokes = strokesFor([], [], points, { lane: 'filter', startMs, endMs }).filter(
      (stroke) => stroke.to[0] < CANVAS_W - 20
    );
    lanes = previous;
    return strokes.filter((stroke) => stroke.color === '#ffffff');
  }

  it('lights each move up to where the next one starts, with no overlap', () => {
    const left = litRuns(0, 3000);
    const right = litRuns(3100, 9000);

    expect(left).not.toHaveLength(0);
    expect(right).not.toHaveLength(0);
    const leftEnd = Math.max(...left.map((stroke) => Math.max(stroke.from[0], stroke.to[0])));
    const rightStart = Math.min(...right.map((stroke) => Math.min(stroke.from[0], stroke.to[0])));
    expect(rightStart).toBeGreaterThanOrEqual(leftEnd);
  });
});

describe('a move whose drop was recorded on its last millisecond', () => {
  it('lights the move but not the flat line the drop leaves behind', () => {
    const previous = lanes;
    lanes = [{ key: 'filter', top: 100, height: 80 }];
    const strokes = strokesFor(
      [],
      [],
      [
        { ms: 1000, value: 0.3 },
        { ms: 4000, value: 0.9 },
        { ms: 4000, value: 0 },
        { ms: 9000, value: 0 }
      ],
      { lane: 'filter', startMs: 1000, endMs: 4000 }
    ).filter((stroke) => stroke.to[0] < CANVAS_W - 20);
    lanes = previous;

    const lit = strokes.filter((stroke) => stroke.color === '#ffffff');
    const tail = strokes.filter((stroke) => stroke.from[0] > lit[lit.length - 1].to[0]);
    expect(lit).not.toHaveLength(0);
    for (const stroke of tail) expect(stroke.color).not.toBe('#ffffff');
  });
});
