import { describe, it, expect } from 'vitest';
import { reorderShift } from '../reorderShift';

describe('a reorder previews itself in place', () => {
  it('carries the dragged row exactly to its target index', () => {
    expect(reorderShift(1, 1, 3)).toBe(2);
    expect(reorderShift(4, 4, 1)).toBe(-3);
    expect(reorderShift(2, 2, 2)).toBe(0);
  });

  it('lifts the rows a downward drag passes', () => {
    expect(reorderShift(2, 1, 3)).toBe(-1);
    expect(reorderShift(3, 1, 3)).toBe(-1);
    expect(reorderShift(4, 1, 3)).toBe(0);
    expect(reorderShift(0, 1, 3)).toBe(0);
  });

  it('drops the rows an upward drag passes', () => {
    expect(reorderShift(1, 4, 1)).toBe(1);
    expect(reorderShift(3, 4, 1)).toBe(1);
    expect(reorderShift(0, 4, 1)).toBe(0);
    expect(reorderShift(5, 4, 1)).toBe(0);
  });

  it('moves nothing when the row is already at its target', () => {
    for (const index of [0, 1, 2, 3]) {
      expect(reorderShift(index, 2, 2)).toBe(0);
    }
  });

  it('never leaves two rows sharing a slot', () => {
    const COUNT = 6;
    for (let from = 0; from < COUNT; from++) {
      for (let target = 0; target < COUNT; target++) {
        const slots = Array.from(
          { length: COUNT },
          (_, index) => index + reorderShift(index, from, target)
        );
        expect(new Set(slots).size, `from ${from} target ${target}: ${slots}`).toBe(COUNT);
      }
    }
  });
});
