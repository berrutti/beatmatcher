import { describe, it, expect } from 'vitest';
import { shareFractions, resizeShareDelta } from '../columnShares';

type Field = 'a' | 'b' | 'c';

describe('shareFractions', () => {
  it('divides fields proportionally to their share', () => {
    const shares: Record<Field, number> = { a: 100, b: 100, c: 200 };
    const result = shareFractions(['a', 'b', 'c'], (f) => shares[f]);
    expect(result.a).toBeCloseTo(0.25);
    expect(result.b).toBeCloseTo(0.25);
    expect(result.c).toBeCloseTo(0.5);
  });

  it('always sums to exactly 1', () => {
    const shares: Record<Field, number> = { a: 37, b: 91, c: 12 };
    const result = shareFractions(['a', 'b', 'c'], (f) => shares[f]);
    expect(result.a + result.b + result.c).toBeCloseTo(1);
  });

  it('gives a single field the whole fraction', () => {
    const shares: Record<Field, number> = { a: 50, b: 0, c: 0 };
    const result = shareFractions(['a'], (f) => shares[f]);
    expect(result.a).toBeCloseTo(1);
  });

  it('returns an empty record for an empty field list without throwing', () => {
    const result = shareFractions([], () => 0);
    expect(result).toEqual({});
  });

  it('returns zero for every field when every share is zero', () => {
    const shares: Record<Field, number> = { a: 0, b: 0, c: 0 };
    const result = shareFractions(['a', 'b', 'c'], (f) => shares[f]);
    expect(result).toEqual({ a: 0, b: 0, c: 0 });
  });
});

describe('resizeShareDelta', () => {
  function setup(shares: Record<Field, number>) {
    return (field: Field, neighbor: Field, deltaPx: number, availableWidth = 800, minPx = 40) =>
      resizeShareDelta({
        fields: ['a', 'b', 'c'],
        getShare: (f) => shares[f],
        field,
        neighbor,
        deltaPx,
        availableWidth,
        minPx
      });
  }

  it('moves share from the neighbor to the dragged field when dragging right (positive delta)', () => {
    const shares: Record<Field, number> = { a: 100, b: 100, c: 200 };
    const resize = setup(shares);
    const { field, neighbor } = resize('a', 'b', 80);
    expect(field).toBeGreaterThan(shares.a);
    expect(neighbor).toBeLessThan(shares.b);
  });

  it('moves share the other way when dragging left (negative delta)', () => {
    const shares: Record<Field, number> = { a: 100, b: 100, c: 200 };
    const resize = setup(shares);
    const { field, neighbor } = resize('a', 'b', -80);
    expect(field).toBeLessThan(shares.a);
    expect(neighbor).toBeGreaterThan(shares.b);
  });

  it('conserves the combined share of the pair exactly', () => {
    const shares: Record<Field, number> = { a: 100, b: 100, c: 200 };
    const resize = setup(shares);
    const { field, neighbor } = resize('a', 'b', 37);
    expect(field + neighbor).toBeCloseTo(shares.a + shares.b);
  });

  it('never shrinks the dragged field below the pixel floor', () => {
    const shares: Record<Field, number> = { a: 100, b: 100, c: 200 };
    const resize = setup(shares);
    const { field } = resize('a', 'b', -100000);
    const total = shares.a + shares.b + shares.c;
    const resultingPx = (field / total) * 800;
    expect(resultingPx).toBeGreaterThanOrEqual(40 - 0.01);
  });

  it('never shrinks the neighbor below the pixel floor', () => {
    const shares: Record<Field, number> = { a: 100, b: 100, c: 200 };
    const resize = setup(shares);
    const { neighbor } = resize('a', 'b', 100000);
    const total = shares.a + shares.b + shares.c;
    const resultingPx = (neighbor / total) * 800;
    expect(resultingPx).toBeGreaterThanOrEqual(40 - 0.01);
  });

  it('leaves fields outside the dragged pair unaffected by construction (only field/neighbor are returned)', () => {
    const shares: Record<Field, number> = { a: 100, b: 100, c: 200 };
    const resize = setup(shares);
    const result = resize('a', 'b', 50);
    expect(Object.keys(result).sort()).toEqual(['field', 'neighbor'].sort());
  });

  it('splits the pair evenly rather than producing an inverted range when neither side can meet the floor', () => {
    const shares: Record<Field, number> = { a: 5, b: 5, c: 990 };
    const resize = setup(shares);
    const { field, neighbor } = resize('a', 'b', 100000);
    expect(field).toBeCloseTo(5, 0);
    expect(neighbor).toBeCloseTo(5, 0);
    expect(field + neighbor).toBeCloseTo(10);
  });

  it("tracks the cursor 1:1: the dragged column's displayed pixel width grows by exactly deltaPx", () => {
    const shares: Record<Field, number> = { a: 100, b: 100, c: 200 };
    const availableWidth = 800;
    const before = shareFractions(['a', 'b', 'c'], (f) => shares[f]);
    const beforePx = before.a * availableWidth;

    const { field: newAShare, neighbor: newBShare } = resizeShareDelta({
      fields: ['a', 'b', 'c'],
      getShare: (f) => shares[f],
      field: 'a',
      neighbor: 'b',
      deltaPx: 37,
      availableWidth,
      minPx: 40
    });
    const after = shareFractions(['a', 'b', 'c'], (f) =>
      f === 'a' ? newAShare : f === 'b' ? newBShare : shares[f]
    );
    const afterPx = after.a * availableWidth;

    expect(afterPx - beforePx).toBeCloseTo(37, 5);
    // The uninvolved column's displayed width is untouched by construction:
    // the pair's combined share (and therefore the total) never changes.
    expect(after.c * availableWidth).toBeCloseTo(before.c * availableWidth, 5);
  });

  it('applying a sequence of small incremental deltas matches one large delta (no drift)', () => {
    const shares: Record<Field, number> = { a: 100, b: 100, c: 200 };
    let current = { ...shares };
    for (let i = 0; i < 10; i++) {
      const { field, neighbor } = resizeShareDelta({
        fields: ['a', 'b', 'c'],
        getShare: (f) => current[f],
        field: 'a',
        neighbor: 'b',
        deltaPx: 8,
        availableWidth: 800,
        minPx: 40
      });
      current = { ...current, a: field, b: neighbor };
    }
    const oneShot = resizeShareDelta({
      fields: ['a', 'b', 'c'],
      getShare: (f) => shares[f],
      field: 'a',
      neighbor: 'b',
      deltaPx: 80,
      availableWidth: 800,
      minPx: 40
    });
    expect(current.a).toBeCloseTo(oneShot.field, 5);
  });
});
