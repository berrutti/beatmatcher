import { describe, it, expect } from 'vitest';
import { distributeColumnWidths } from '../columnLayout';

type Field = 'a' | 'b' | 'c';

const widths: Record<Field, number> = { a: 100, b: 100, c: 200 };
function getWidth(field: Field): number {
  return widths[field];
}

describe('distributeColumnWidths', () => {
  it('returns exactly the configured widths when there is no leftover space', () => {
    const result = distributeColumnWidths(400, 0, ['a', 'b', 'c'], getWidth);
    expect(result).toEqual({ a: 100, b: 100, c: 200 });
  });

  it('returns exactly the configured widths when the container is smaller than the basis', () => {
    const result = distributeColumnWidths(200, 0, ['a', 'b', 'c'], getWidth);
    expect(result).toEqual({ a: 100, b: 100, c: 200 });
  });

  it('never shrinks a column below its configured width, even with a huge fixed total', () => {
    const result = distributeColumnWidths(100, 10000, ['a', 'b', 'c'], getWidth);
    expect(result).toEqual({ a: 100, b: 100, c: 200 });
  });

  it('distributes leftover space proportionally to each column share of the basis', () => {
    // basis = 400, fixedTotal = 50, container = 650 -> extra = 200
    const result = distributeColumnWidths(650, 50, ['a', 'b', 'c'], getWidth);
    // a and b each hold 1/4 of the basis, c holds 1/2
    expect(result.a).toBeCloseTo(150);
    expect(result.b).toBeCloseTo(150);
    expect(result.c).toBeCloseTo(300);
  });

  it('conserves total width: fixedTotal + sum(result) equals containerWidth when there is leftover space', () => {
    const fixedTotal = 80;
    const containerWidth = 900;
    const result = distributeColumnWidths(containerWidth, fixedTotal, ['a', 'b', 'c'], getWidth);
    const sum = result.a + result.b + result.c;
    expect(sum + fixedTotal).toBeCloseTo(containerWidth);
  });

  it('gives a single column all the leftover space', () => {
    const result = distributeColumnWidths(500, 0, ['a'], getWidth);
    expect(result.a).toBeCloseTo(500);
  });

  it('returns an empty object for an empty field list without throwing', () => {
    const result = distributeColumnWidths(500, 0, [], getWidth);
    expect(result).toEqual({});
  });

  it('splits leftover space evenly when every configured width is zero', () => {
    const zeroWidths: Record<Field, number> = { a: 0, b: 0, c: 0 };
    const result = distributeColumnWidths(300, 0, ['a', 'b', 'c'], (field) => zeroWidths[field]);
    expect(result.a).toBeCloseTo(100);
    expect(result.b).toBeCloseTo(100);
    expect(result.c).toBeCloseTo(100);
  });

  it('is a pure function: repeated calls with the same input return the same output', () => {
    const first = distributeColumnWidths(650, 50, ['a', 'b', 'c'], getWidth);
    const second = distributeColumnWidths(650, 50, ['a', 'b', 'c'], getWidth);
    expect(first).toEqual(second);
  });
});
