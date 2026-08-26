import { describe, it, expect } from 'vitest';
import { drawJogLane, LABEL_W, laneValuePad } from '@renderer/utils/timelineDraw';
import type { LanePoint } from '@renderer/utils/types';

type Bar = { x: number; y: number; w: number; h: number };

const LANE_Y = 100;
const LANE_H = 64;
const CANVAS_W = 500;
const CENTER_Y = LANE_Y + LANE_H / 2;
const TRACK_W = CANVAS_W - LABEL_W - 12;
const VIEW_MS = 2000;

// The frame and the centre line span the whole track. Plotted data is one column wide.
function valueBars(bars: Bar[]): Bar[] {
  return bars.filter((bar) => bar.w === 1);
}

const SCALE_PCT = 16;

function draw(curve: LanePoint[], scale = SCALE_PCT): Bar[] {
  const bars: Bar[] = [];
  const ctx = {
    fillRect: (x: number, y: number, w: number, h: number) => bars.push({ x, y, w, h }),
    fillText: () => {},
    save: () => {},
    restore: () => {},
    set globalAlpha(_value: number) {},
    set fillStyle(_value: string) {},
    set font(_value: string) {},
    set textAlign(_value: string) {},
    set textBaseline(_value: string) {}
  } as unknown as CanvasRenderingContext2D;
  const xToMs = (x: number) => ((x - LABEL_W) / TRACK_W) * VIEW_MS;
  drawJogLane(
    ctx,
    CANVAS_W,
    LANE_Y,
    LANE_H,
    curve,
    xToMs,
    scale,
    [],
    new Map(),
    () => 0,
    '#ffffff'
  );
  return valueBars(bars);
}

const GESTURE: LanePoint[] = [
  { ms: 0, value: 0 },
  { ms: 500, value: SCALE_PCT },
  { ms: 900, value: 0 }
];

describe('drawJogLane', () => {
  it('draws forward travel above the centre and reverse below', () => {
    const forward = draw(GESTURE);
    const reverse = draw(GESTURE.map((point) => ({ ...point, value: -point.value })));

    expect(forward.length).toBeGreaterThan(0);
    expect(reverse.length).toBeGreaterThan(0);
    expect(forward.every((bar) => bar.y + bar.h <= CENTER_Y)).toBe(true);
    expect(reverse.every((bar) => bar.y >= CENTER_Y)).toBe(true);
  });

  it('plots a value at the scale as the full half-height', () => {
    const tallest = Math.max(...draw(GESTURE).map((bar) => bar.h));

    expect(tallest).toBeCloseTo(LANE_H / 2 - laneValuePad(LANE_H), 9);
  });

  it('clips a recorded spike past the scale rather than shrinking the rest', () => {
    const spike = GESTURE.map((point) => ({ ...point, value: point.value * 4 }));
    const tallest = Math.max(...draw(spike).map((bar) => bar.h));

    expect(tallest).toBeCloseTo(LANE_H / 2 - laneValuePad(LANE_H), 9);
  });

  it('plots nothing for a deck that never touched the wheel', () => {
    expect(draw([{ ms: 0, value: 0 }])).toHaveLength(0);
  });
});
