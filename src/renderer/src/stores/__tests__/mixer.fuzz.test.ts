import { describe, it, expect, vi, beforeEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({})
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {})
}));

vi.mock('@tauri-apps/plugin-store', () => ({
  load: vi.fn().mockResolvedValue({})
}));

vi.mock('@renderer/utils/storage', () => ({
  storageGet: vi.fn((_key: string, fallback: unknown) => fallback),
  storageSet: vi.fn(),
  STORAGE_KEYS: { deckCount: 'deckCount', collectionHeight: 'collectionHeight' }
}));

vi.mock('@renderer/stores/settings', () => ({
  DEFAULT_MIXER_ID: 'classic-3band',
  LIVE_MIXER_ID: 'classic-3band-v2',
  useSettingsStore: () => ({
    pitchRange: 8,
    nudgeSensitivity: 4,
    recordingBitDepth: 24,
    recordingFormat: 'wav',
    recordSession: false
  })
}));

import { useMixerStore, paramKey } from '../mixer';
import { mixerParams } from '@renderer/utils/sessionCore';
import { LIVE_MIXER_ID } from '@renderer/stores/settings';
import { invoke } from '@tauri-apps/api/core';
import type { DeckId } from '@renderer/utils/types';

const mockedInvoke = vi.mocked(invoke);

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

// Well past every range, so the clamps are under test rather than the caller.
function wildValue(random: () => number): number {
  const magnitude = [1, 10, 100, 1e6][Math.floor(random() * 4)];
  return (random() * 2 - 1) * magnitude;
}

const DECKS: DeckId[] = ['A', 'B', 'C', 'D'];
const BANDS = ['low', 'mid', 'high'] as const;

function rangeOf(slot: string, param: string): { min: number; max: number } {
  const spec = mixerParams(LIVE_MIXER_ID)[`${slot}/${param}`];
  if (!spec) throw new Error(`${slot}/${param} is not on ${LIVE_MIXER_ID}`);
  return { min: spec.min, max: spec.max };
}

describe('mixer writes under fuzzed input', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('keeps every eq band inside the manifest range, in state and on the wire', () => {
    const store = useMixerStore();
    const random = makeRandom(7);
    const { min, max } = rangeOf('eq', 'low');

    for (let step = 0; step < 3000; step++) {
      const deck = DECKS[Math.floor(random() * DECKS.length)];
      const band = BANDS[Math.floor(random() * BANDS.length)];
      const key = paramKey('eq', band);
      store.setParam(deck, key, wildValue(random));

      expect(store.paramValue(deck, key), `step ${step}`).toBeGreaterThanOrEqual(min);
      expect(store.paramValue(deck, key), `step ${step}`).toBeLessThanOrEqual(max);
      const last = mockedInvoke.mock.calls[mockedInvoke.mock.calls.length - 1];
      const payload = last[1] as { value: number };
      expect(payload.value, `step ${step}`).toBe(store.paramValue(deck, key));
    }
  });

  // Every deck-scope address, looped from the manifest, so a param the mixer
  // gains is fuzzed without this test naming it.
  it('keeps every manifest param and the crossfader inside their ranges', () => {
    const store = useMixerStore();
    const random = makeRandom(11);
    const specs = Object.values(mixerParams(LIVE_MIXER_ID));

    for (let step = 0; step < 3000; step++) {
      const deck = DECKS[Math.floor(random() * DECKS.length)];
      store.setXfaderPosition(wildValue(random));
      store.setMasterGain(wildValue(random));
      store.setCueMix(wildValue(random));

      for (const spec of specs) {
        const key = paramKey(spec.slot, spec.param);
        store.setParam(deck, key, wildValue(random));
        expect(store.paramValue(deck, key), `step ${step} ${key}`).toBeGreaterThanOrEqual(spec.min);
        expect(store.paramValue(deck, key), `step ${step} ${key}`).toBeLessThanOrEqual(spec.max);
      }
      expect(Math.abs(store.xfaderPosition), `step ${step}`).toBeLessThanOrEqual(1);
      expect(store.masterGain, `step ${step}`).toBeGreaterThanOrEqual(0);
      expect(store.masterGain, `step ${step}`).toBeLessThanOrEqual(1);
      expect(store.cueMix, `step ${step}`).toBeGreaterThanOrEqual(0);
      expect(store.cueMix, `step ${step}`).toBeLessThanOrEqual(1);
    }
  });
});
