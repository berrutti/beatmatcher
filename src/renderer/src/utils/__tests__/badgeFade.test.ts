import { describe, it, expect } from 'vitest';
import {
  BADGE_FADE_MS,
  badgeAlpha,
  badgeFading,
  updateBadgeFade,
  type BadgeFade
} from '@renderer/utils/badgeFade';

const MUTE = { label: 'MUTE', solo: false };
const SOLO = { label: 'SOLO', solo: true };

describe('updateBadgeFade', () => {
  it('starts hidden and fully faded, so a first render does not animate', () => {
    const fade = updateBadgeFade(undefined, null, 1000);
    expect(badgeAlpha(fade, 1000)).toBe(0);
    expect(badgeFading(fade, 1000)).toBe(false);
  });

  it('stamps the moment a badge appears', () => {
    const fade = updateBadgeFade(updateBadgeFade(undefined, null, 1000), MUTE, 1200);
    expect(badgeAlpha(fade, 1200)).toBe(0);
    expect(badgeAlpha(fade, 1200 + BADGE_FADE_MS)).toBe(1);
  });

  it('keeps the same stamp while nothing changes, so the fade runs once', () => {
    const shown = updateBadgeFade(undefined, MUTE, 1000);
    const again = updateBadgeFade(shown, MUTE, 1050);
    expect(again.changedAtMs).toBe(shown.changedAtMs);
  });

  it('holds the badge on screen while it fades out', () => {
    const shown = updateBadgeFade(undefined, MUTE, 1000);
    const hiding = updateBadgeFade(shown, null, 2000);
    expect(hiding.badge).toEqual(MUTE);
    expect(badgeAlpha(hiding, 2000)).toBe(1);
    expect(badgeAlpha(hiding, 2000 + BADGE_FADE_MS)).toBe(0);
  });

  it('swaps mute for solo without restarting the fade, since both are already on screen', () => {
    const muted = updateBadgeFade(undefined, MUTE, 1000);
    const soloed = updateBadgeFade(muted, SOLO, 2000);
    expect(soloed.badge).toEqual(SOLO);
    expect(soloed.changedAtMs).toBe(muted.changedAtMs);
  });
});

describe('badgeFading', () => {
  it('reports true only until the fade is over, so the canvas stops redrawing', () => {
    const fade: BadgeFade = updateBadgeFade(undefined, MUTE, 1000);
    expect(badgeFading(fade, 1000)).toBe(true);
    expect(badgeFading(fade, 1000 + BADGE_FADE_MS / 2)).toBe(true);
    expect(badgeFading(fade, 1000 + BADGE_FADE_MS)).toBe(false);
  });
});

describe('badgeAlpha', () => {
  it('eases rather than ramping, so the badge does not pop', () => {
    const fade = updateBadgeFade(undefined, MUTE, 0);
    expect(badgeAlpha(fade, BADGE_FADE_MS / 4)).toBeLessThan(0.25);
    expect(badgeAlpha(fade, (BADGE_FADE_MS * 3) / 4)).toBeGreaterThan(0.75);
  });
});
