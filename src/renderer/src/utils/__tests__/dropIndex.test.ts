import { describe, it, expect } from 'vitest';
import { targetIndexAt } from '../dropIndex';

const CONTENT_TOP = 0;
const ROW = 32;
const COUNT = 6;

// Where the pointer sits inside the row when the drag starts.
const AT_CENTER = ROW / 2;
const AT_TOP = 2;
const AT_BOTTOM = ROW - 2;

function indexFor(fromIdx: number, grabOffset: number, movedRows: number): number {
  const pointerY = CONTENT_TOP + fromIdx * ROW + grabOffset + movedRows * ROW;
  return targetIndexAt(pointerY, grabOffset, CONTENT_TOP, ROW, COUNT);
}

describe('a dragged row lands under the point it was picked up by', () => {
  it('stays put while the pointer has not left the row', () => {
    expect(indexFor(2, AT_CENTER, 0)).toBe(2);
    expect(indexFor(2, AT_TOP, 0)).toBe(2);
    expect(indexFor(2, AT_BOTTOM, 0)).toBe(2);
  });

  it('moves one row for one row of travel, in either direction', () => {
    // The old boundary-based index needed a full row down but a row and a half
    // up, so the row landed above the pointer.
    expect(indexFor(3, AT_CENTER, 1)).toBe(4);
    expect(indexFor(3, AT_CENTER, -1)).toBe(2);
  });

  it('does not care where in the row the drag was started from', () => {
    for (const grab of [AT_TOP, AT_CENTER, AT_BOTTOM]) {
      expect(indexFor(3, grab, 1), `grabbed at ${grab}`).toBe(4);
      expect(indexFor(3, grab, -1), `grabbed at ${grab}`).toBe(2);
      expect(indexFor(3, grab, 2), `grabbed at ${grab}`).toBe(5);
    }
  });

  it('holds still until the travel is past half a row', () => {
    expect(indexFor(3, AT_CENTER, 0.49)).toBe(3);
    expect(indexFor(3, AT_CENTER, -0.49)).toBe(3);
  });

  it('clamps to the list rather than running off it', () => {
    expect(indexFor(0, AT_CENTER, -20)).toBe(0);
    expect(indexFor(COUNT - 1, AT_CENTER, 20)).toBe(COUNT - 1);
  });

  it('answers with no row height rather than dividing by zero', () => {
    expect(targetIndexAt(100, AT_CENTER, CONTENT_TOP, 0, COUNT)).toBe(0);
  });
});
