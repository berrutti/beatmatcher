import { describe, it, expect } from 'vitest';
import {
  BADGE_FADE_MS,
  badgeAlpha,
  badgeFading,
  updateBadgeFade,
  type Badge,
  type BadgeFade
} from '@renderer/utils/badgeFade';

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

const MUTE: Badge = { label: 'MUTE', solo: false };
const SOLO: Badge = { label: 'SOLO', solo: true };

describe('the badge fade under fuzzed toggling', () => {
  it('stays a drawable alpha however fast mute and solo are hammered', () => {
    const random = makeRandom(43);
    let fade: BadgeFade | undefined;
    let now = 0;
    for (let step = 0; step < 5000; step++) {
      now += random() * 300;
      const roll = random();
      const badge = roll < 0.4 ? null : roll < 0.7 ? MUTE : SOLO;
      fade = updateBadgeFade(fade, badge, now);
      const alpha = badgeAlpha(fade, now);
      expect(alpha).toBeGreaterThanOrEqual(0);
      expect(alpha).toBeLessThanOrEqual(1);
    }
  });

  it('always settles, so the canvas stops asking for frames', () => {
    const random = makeRandom(47);
    let fade: BadgeFade | undefined;
    let now = 0;
    for (let step = 0; step < 2000; step++) {
      now += random() * 500;
      fade = updateBadgeFade(fade, random() < 0.5 ? null : MUTE, now);
      expect(badgeFading(fade, now + BADGE_FADE_MS)).toBe(false);
      expect(badgeAlpha(fade, now + BADGE_FADE_MS)).toBe(fade.visible ? 1 : 0);
    }
  });

  it('keeps a badge to draw whenever it is not fully faded out', () => {
    const random = makeRandom(53);
    let fade: BadgeFade | undefined;
    let now = 0;
    for (let step = 0; step < 3000; step++) {
      now += random() * 200;
      fade = updateBadgeFade(fade, random() < 0.5 ? null : SOLO, now);
      if (badgeAlpha(fade, now) > 0) expect(fade.badge).not.toBeNull();
    }
  });
});
