import { describe, it, expect } from 'vitest';
import { DECK_LANE_KEYS } from '@renderer/utils/types';
import en from '@renderer/locales/en.json';

describe('the locale names every lane', () => {
  it('has a distinct name for each, so a row cannot be mistaken for another', () => {
    const names = DECK_LANE_KEYS.map((key) => en.session.lanes[key]);
    for (const [key, name] of DECK_LANE_KEYS.map((key, i) => [key, names[i]] as const)) {
      expect(name, `no name for ${key}`).toBeTruthy();
    }
    expect(new Set(names).size).toBe(names.length);
  });
});
