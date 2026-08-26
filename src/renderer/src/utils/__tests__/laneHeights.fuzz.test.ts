import { describe, it, expect } from 'vitest';
import {
  DEFAULT_LANE_HEIGHT,
  DEFAULT_WAVEFORM_HEIGHT,
  MIN_LANE_HEIGHT,
  MIN_WAVEFORM_HEIGHT,
  laneHeightFor,
  waveformHeightFor,
  withLaneHeight,
  withWaveformHeight,
  type StoredLaneHeights
} from '@renderer/utils/laneHeights';
import { DECK_LANE_KEYS } from '@renderer/utils/types';
import type { DeckLaneKey } from '@renderer/utils/laneSelection';

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

const DECKS = ['A', 'B', 'C', 'D'];
const MAX_SANE_HEIGHT = 240;

describe('lane heights under fuzzed drags', () => {
  it('never stores a height that would hide a row or fill the timeline', () => {
    const random = makeRandom(3);
    let stored: StoredLaneHeights = {};
    for (let step = 0; step < 2000; step++) {
      const deck = DECKS[Math.floor(random() * DECKS.length)];
      const lane = DECK_LANE_KEYS[Math.floor(random() * DECK_LANE_KEYS.length)];
      const height = (random() - 0.3) * 4000;
      stored =
        random() < 0.5
          ? withLaneHeight(stored, deck, lane, height)
          : withWaveformHeight(stored, deck, height);

      for (const readDeck of DECKS) {
        expect(waveformHeightFor(stored, readDeck)).toBeGreaterThanOrEqual(MIN_WAVEFORM_HEIGHT);
        expect(waveformHeightFor(stored, readDeck)).toBeLessThanOrEqual(MAX_SANE_HEIGHT);
        for (const readLane of DECK_LANE_KEYS) {
          const read = laneHeightFor(stored, readDeck, readLane);
          expect(read).toBeGreaterThanOrEqual(MIN_LANE_HEIGHT);
          expect(read).toBeLessThanOrEqual(MAX_SANE_HEIGHT);
        }
      }
    }
  });

  it('leaves every other slot untouched, so no two separators move together', () => {
    const random = makeRandom(9);
    let stored: StoredLaneHeights = {};
    for (let step = 0; step < 500; step++) {
      const deck = DECKS[Math.floor(random() * DECKS.length)];
      const lane: DeckLaneKey = DECK_LANE_KEYS[Math.floor(random() * DECK_LANE_KEYS.length)];
      const before = DECKS.flatMap((other) =>
        DECK_LANE_KEYS.map((key) => `${other}:${key}=${laneHeightFor(stored, other, key)}`)
      );

      stored = withLaneHeight(stored, deck, lane, 40 + random() * 100);

      const after = DECKS.flatMap((other) =>
        DECK_LANE_KEYS.map((key) => `${other}:${key}=${laneHeightFor(stored, other, key)}`)
      );
      const moved = before.filter((entry, index) => entry !== after[index]);
      expect(moved.length).toBeLessThanOrEqual(1);
    }
  });

  it('falls back to the defaults for any shape of junk in storage', () => {
    const random = makeRandom(21);
    const junk: unknown[] = [
      null,
      undefined,
      42,
      'tall',
      [],
      { 'A:filter': 'x' },
      { 'A:waveform': NaN }
    ];
    for (let step = 0; step < 500; step++) {
      const stored = junk[Math.floor(random() * junk.length)];
      const read = laneHeightFor(stored, 'A', 'filter');
      expect([DEFAULT_LANE_HEIGHT, MIN_LANE_HEIGHT]).toContain(read);
      expect(waveformHeightFor(stored, 'A')).toBe(DEFAULT_WAVEFORM_HEIGHT);
    }
  });
});
