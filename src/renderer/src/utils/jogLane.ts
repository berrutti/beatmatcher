import type { LanePoint } from '@renderer/utils/types';

// A quiet lane must not amplify a fraction of a percent into a full-height shape.
export const JOG_LANE_MIN_SCALE_PCT = 5;

// Takes each column's extreme rather than its edge: a gesture settles in tens of
// milliseconds, so zoomed out it falls entirely inside one column and would vanish.
export function jogLaneColumns(
  curve: LanePoint[],
  columnCount: number,
  msAtColumn: (column: number) => number
): number[] {
  const columns = new Array<number>(Math.max(0, columnCount)).fill(0);
  let cursor = 0;
  let held = 0;

  for (let column = 0; column < columns.length; column++) {
    const endMs = msAtColumn(column + 1);
    while (cursor < curve.length && curve[cursor].ms <= msAtColumn(column)) {
      held = curve[cursor].value;
      cursor++;
    }
    let peak = held;
    while (cursor < curve.length && curve[cursor].ms < endMs) {
      held = curve[cursor].value;
      if (Math.abs(held) > Math.abs(peak)) peak = held;
      cursor++;
    }
    columns[column] = peak;
  }
  return columns;
}

// Symmetric, so the centre line stays at zero.
export function jogLaneScale(columns: number[]): number {
  let scale = JOG_LANE_MIN_SCALE_PCT;
  for (const value of columns) scale = Math.max(scale, Math.abs(value));
  return scale;
}
