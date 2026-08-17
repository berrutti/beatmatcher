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

const PITCH_RANGE = 8;

vi.mock('@renderer/stores/settings', () => ({
  useSettingsStore: () => ({
    pitchRange: PITCH_RANGE,
    nudgeSensitivity: 4,
    deckAccents: {},
    setDeckAccents: vi.fn()
  })
}));

vi.mock('@renderer/utils/storage', () => ({
  storageGet: vi.fn().mockReturnValue({}),
  storageSet: vi.fn(),
  STORAGE_KEYS: {
    savedTracks: 'savedTracks',
    collection: 'collection',
    collectionHeight: 'collectionHeight'
  }
}));

vi.mock('@renderer/stores/session', () => ({
  useSessionStore: () => ({
    session: null,
    isPlaying: false,
    exit: vi.fn(),
    play: vi.fn(),
    stop: vi.fn()
  })
}));

vi.mock('@renderer/stores/mixer', () => ({
  useMixerStore: () => ({ reset: vi.fn() })
}));

import { useDecksStore } from '../decks';
import { invoke } from '@tauri-apps/api/core';

const mockedInvoke = vi.mocked(invoke);

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

// roundBpm's quantum in decks.ts: the displayed bpm is hundredths.
const BPM_QUANTUM = 0.01;

describe('pitch and bpm under fuzzed input', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('never leaves the pitch range, whatever is asked for', () => {
    const decks = useDecksStore();
    const random = makeRandom(3);

    for (let step = 0; step < 3000; step++) {
      const trackBpm = 40 + random() * 180;
      decks.deckA.setTrackBpm(trackBpm);
      if (random() < 0.5) {
        decks.deckA.setPitchOffset((random() * 2 - 1) * 100);
      } else {
        decks.deckA.setTargetBpm(trackBpm * (random() * 3));
      }

      const { pitchOffset, targetBpm } = decks.deckA;
      expect(Math.abs(pitchOffset), `step ${step}`).toBeLessThanOrEqual(PITCH_RANGE + 1e-9);
      expect(targetBpm, `step ${step}`).not.toBeNull();
      if (targetBpm === null) continue;
      // No slack: the engine's rate is derived from this bpm.
      expect(targetBpm, `step ${step}`).toBeGreaterThanOrEqual(trackBpm * (1 - PITCH_RANGE / 100));
      expect(targetBpm, `step ${step}`).toBeLessThanOrEqual(trackBpm * (1 + PITCH_RANGE / 100));
    }
  });

  it('keeps the offset and the displayed bpm telling the same story', () => {
    const decks = useDecksStore();
    const random = makeRandom(5);

    for (let step = 0; step < 3000; step++) {
      const trackBpm = 40 + random() * 180;
      decks.deckB.setTrackBpm(trackBpm);
      decks.deckB.setPitchOffset((random() * 2 - 1) * PITCH_RANGE);

      const { pitchOffset, targetBpm } = decks.deckB;
      if (targetBpm === null) throw new Error('a loaded grid always has a target');
      const fromOffset = trackBpm * (1 + pitchOffset / 100);
      expect(Math.abs(targetBpm - fromOffset), `step ${step}`).toBeLessThanOrEqual(BPM_QUANTUM);
    }
  });

  it('mirrors an engine rate without invoking back', () => {
    const decks = useDecksStore();
    const random = makeRandom(9);
    decks.deckC.setTrackBpm(128);
    mockedInvoke.mockClear();

    for (let step = 0; step < 3000; step++) {
      const rate = 1 + (random() * 2 - 1) * (PITCH_RANGE / 100);
      decks.deckC.applyEngineRate(rate);

      expect(mockedInvoke, `step ${step}`).not.toHaveBeenCalled();
      const { targetBpm, pitchOffset } = decks.deckC;
      if (targetBpm === null) throw new Error('a loaded grid always has a target');
      expect(Math.abs(targetBpm - 128 * rate), `step ${step}`).toBeLessThanOrEqual(BPM_QUANTUM);
      expect(Math.abs(pitchOffset - (rate - 1) * 100), `step ${step}`).toBeLessThan(1e-9);
    }
  });

  it('leaves a deck with no grid alone', () => {
    const decks = useDecksStore();
    const random = makeRandom(13);

    for (let step = 0; step < 500; step++) {
      decks.deckD.setPitchOffset((random() * 2 - 1) * 100);
      decks.deckD.setTargetBpm(random() * 300);
      decks.deckD.applyEngineRate(1 + random());

      expect(decks.deckD.trackBpm, `step ${step}`).toBeNull();
      expect(decks.deckD.targetBpm, `step ${step}`).toBeNull();
      expect(decks.deckD.pitchOffset, `step ${step}`).toBe(0);
    }
  });
});
