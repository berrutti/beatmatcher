import { describe, it, expect } from 'vitest';
import {
  jogLaneColumns,
  jogLaneScale,
  nudgeSpanAt,
  JOG_LANE_MIN_SCALE_PCT
} from '@renderer/utils/jogLane';
import type { LanePoint } from '@renderer/utils/types';

const COLUMNS = 10;
const MS_PER_COLUMN = 100;
const msAtColumn = (column: number) => column * MS_PER_COLUMN;

describe('jogLaneColumns', () => {
  it('holds a step value across every column it spans', () => {
    const curve: LanePoint[] = [
      { ms: 0, value: 0 },
      { ms: 200, value: 4 },
      { ms: 500, value: 0 }
    ];

    expect(jogLaneColumns(curve, COLUMNS, msAtColumn)).toEqual([0, 0, 4, 4, 4, 0, 0, 0, 0, 0]);
  });

  it('keeps a spike that is narrower than one column', () => {
    const curve: LanePoint[] = [
      { ms: 0, value: 0 },
      { ms: 320, value: 60 },
      { ms: 325, value: 0 }
    ];

    expect(jogLaneColumns(curve, COLUMNS, msAtColumn)[3]).toBe(60);
  });

  it('takes the extreme of a column, not the last value in it', () => {
    const curve: LanePoint[] = [
      { ms: 0, value: 0 },
      { ms: 210, value: 30 },
      { ms: 240, value: 2 },
      { ms: 270, value: 1 }
    ];

    expect(jogLaneColumns(curve, COLUMNS, msAtColumn)[2]).toBe(30);
  });

  it('reads a reverse spike as the extreme over a smaller forward one', () => {
    const curve: LanePoint[] = [
      { ms: 0, value: 0 },
      { ms: 210, value: 5 },
      { ms: 240, value: -40 },
      { ms: 260, value: 0 }
    ];

    expect(jogLaneColumns(curve, COLUMNS, msAtColumn)[2]).toBe(-40);
  });

  it('is all zero for a deck that never touched the wheel', () => {
    expect(jogLaneColumns([], COLUMNS, msAtColumn)).toEqual(new Array(COLUMNS).fill(0));
  });
});

describe('jogLaneScale', () => {
  it('floors at the minimum so a still lane is not amplified', () => {
    expect(jogLaneScale([0, 0.2, -0.1])).toBe(JOG_LANE_MIN_SCALE_PCT);
  });

  it('grows to the largest excursion in either direction', () => {
    expect(jogLaneScale([4, -37, 2])).toBe(37);
  });
});

describe('nudgeSpanAt', () => {
  const spans = [
    { startMs: 100, endMs: 200 },
    { startMs: 400, endMs: 900 }
  ];

  it('finds the span under the cursor', () => {
    expect(nudgeSpanAt(spans, 500)).toEqual(spans[1]);
  });

  it('is null between spans', () => {
    expect(nudgeSpanAt(spans, 300)).toBeNull();
  });
});
