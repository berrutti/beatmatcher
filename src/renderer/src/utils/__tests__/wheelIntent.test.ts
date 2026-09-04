import { describe, it, expect } from 'vitest';
import { wheelIntent } from '../wheelIntent';

const swipe = (deltaX: number, deltaY: number) => ({
  ctrlKey: false,
  metaKey: false,
  deltaX,
  deltaY
});

describe('wheelIntent', () => {
  it('reads a two-finger swipe as a pan whichever way it leans', () => {
    expect(wheelIntent(swipe(40, 6))).toBe('pan');
    expect(wheelIntent(swipe(6, 40))).toBe('pan');
    expect(wheelIntent(swipe(0, 40))).toBe('pan');
  });

  it('reads the ends of a swipe as a pan, where the deltas go small and wander', () => {
    expect(wheelIntent(swipe(0.5, 1))).toBe('pan');
    expect(wheelIntent(swipe(1.5, 0.5))).toBe('pan');
  });

  it('reads a pinch as a zoom whatever its deltas say', () => {
    expect(wheelIntent({ ctrlKey: true, metaKey: false })).toBe('zoom');
    expect(wheelIntent({ ctrlKey: false, metaKey: true })).toBe('zoom');
  });
});
