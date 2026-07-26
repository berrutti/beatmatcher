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
  useSettingsStore: () => ({
    pitchRange: 8,
    nudgeSensitivity: 4,
    recordingBitDepth: 24,
    recordingFormat: 'wav',
    recordSession: false
  })
}));

import { useMixerStore } from '../mixer';
import { editConstants } from '@renderer/utils/sessionCore';

const { eqMinDb: EQ_MIN_DB, eqMaxDb: EQ_MAX_DB } = editConstants();
import { invoke } from '@tauri-apps/api/core';

const mockedInvoke = vi.mocked(invoke);

describe('setEq', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('updates eq state and invokes set_eq', () => {
    const store = useMixerStore();
    store.setEq('A', 'high', 3);
    expect(store.eq.A.high).toBe(3);
    expect(mockedInvoke).toHaveBeenCalledWith('set_eq', { deck: 'A', band: 'high', db: 3 });
  });

  it('clamps below EQ_MIN_DB', () => {
    const store = useMixerStore();
    store.setEq('B', 'low', EQ_MIN_DB - 10);
    expect(store.eq.B.low).toBe(EQ_MIN_DB);
    expect(mockedInvoke).toHaveBeenCalledWith('set_eq', { deck: 'B', band: 'low', db: EQ_MIN_DB });
  });

  it('clamps above EQ_MAX_DB', () => {
    const store = useMixerStore();
    store.setEq('C', 'mid', EQ_MAX_DB + 10);
    expect(store.eq.C.mid).toBe(EQ_MAX_DB);
    expect(mockedInvoke).toHaveBeenCalledWith('set_eq', { deck: 'C', band: 'mid', db: EQ_MAX_DB });
  });

  it('does not affect other decks or bands', () => {
    const store = useMixerStore();
    store.setEq('A', 'low', 4);
    expect(store.eq.A.mid).toBe(0);
    expect(store.eq.A.high).toBe(0);
    expect(store.eq.B.low).toBe(0);
  });
});

// The sliders bind to these, so a descriptor change has to reach the UI rather
// than the store restating the range.
describe('eq specs', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('come from the mixer manifest', () => {
    const store = useMixerStore();
    expect(store.eqSpecs.map((spec) => spec.param)).toEqual(['low', 'mid', 'high']);
    for (const spec of store.eqSpecs) {
      expect(spec.min).toBe(EQ_MIN_DB);
      expect(spec.max).toBe(EQ_MAX_DB);
      expect(spec.defaultValue).toBe(0);
      expect(spec.step).toBeGreaterThan(0);
    }
  });
});

describe('reset', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('resets volume to 1 for all live decks', () => {
    const store = useMixerStore();
    store.setVolume('A', 0.3);
    store.setVolume('B', 0.5);
    vi.clearAllMocks();

    store.reset();

    expect(store.volume.A).toBe(1);
    expect(store.volume.B).toBe(1);
    expect(store.volume.C).toBe(1);
    expect(store.volume.D).toBe(1);
  });

  it('resets all EQ bands to the mixer default for all live decks', () => {
    const store = useMixerStore();
    store.setEq('A', 'low', 5);
    store.setEq('B', 'high', -10);
    vi.clearAllMocks();

    store.reset();

    for (const deckId of ['A', 'B', 'C', 'D'] as const) {
      expect(store.eq[deckId].low).toBe(store.eqDefault('low'));
      expect(store.eq[deckId].mid).toBe(store.eqDefault('mid'));
      expect(store.eq[deckId].high).toBe(store.eqDefault('high'));
    }
  });

  it('resets filter to 0 for all live decks', () => {
    const store = useMixerStore();
    store.setFilter('A', 0.8);
    vi.clearAllMocks();

    store.reset();

    expect(store.filter.A).toBe(0);
    expect(store.filter.B).toBe(0);
  });

  it('resets filterEnabled to false for all live decks', () => {
    const store = useMixerStore();
    store.toggleFilter('A');
    store.toggleFilter('C');
    vi.clearAllMocks();

    store.reset();

    expect(store.filterEnabled.A).toBe(false);
    expect(store.filterEnabled.C).toBe(false);
  });

  it('resets cueActive to false for all live decks', () => {
    const store = useMixerStore();
    store.setCueActive('B', true);
    store.setCueActive('D', true);
    vi.clearAllMocks();

    store.reset();

    expect(store.cueActive.B).toBe(false);
    expect(store.cueActive.D).toBe(false);
  });

  it('invokes Rust setters for all live decks', () => {
    const store = useMixerStore();
    store.reset();

    for (const deck of ['A', 'B', 'C', 'D']) {
      expect(mockedInvoke).toHaveBeenCalledWith('set_volume', { deck, gain: 1 });
      expect(mockedInvoke).toHaveBeenCalledWith('set_eq', { deck, band: 'low', db: 0 });
      expect(mockedInvoke).toHaveBeenCalledWith('set_eq', { deck, band: 'mid', db: 0 });
      expect(mockedInvoke).toHaveBeenCalledWith('set_eq', { deck, band: 'high', db: 0 });
      expect(mockedInvoke).toHaveBeenCalledWith('set_filter', { deck, value: 0 });
      expect(mockedInvoke).toHaveBeenCalledWith('set_filter_active', { deck, active: false });
      expect(mockedInvoke).toHaveBeenCalledWith('set_cue_active', { deck, active: false });
    }
  });

  it('does not touch deck E', () => {
    const store = useMixerStore();
    store.setVolume('E', 0.5);
    vi.clearAllMocks();

    store.reset();

    expect(store.volume.E).toBe(0.5);
    expect(mockedInvoke).not.toHaveBeenCalledWith('set_volume', {
      deck: 'E',
      gain: expect.anything()
    });
  });
});

describe('applyEngineParam', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('mirrors every deck param the mixer holds', () => {
    const store = useMixerStore();

    store.applyEngineParam({ deck: 'A', slot: 'eq', param: 'low', value: -6 });
    store.applyEngineParam({ deck: 'B', slot: 'fader', param: 'gain', value: 0.25 });
    store.applyEngineParam({ deck: 'C', slot: 'filter', param: 'value', value: -0.5 });
    store.applyEngineParam({ deck: 'D', slot: 'filter', param: 'active', value: 1 });

    expect(store.eq.A.low).toBe(-6);
    expect(store.volume.B).toBe(0.25);
    expect(store.filter.C).toBe(-0.5);
    expect(store.filterEnabled.D).toBe(true);
  });

  // The whole point of origin tagging: a push applies locally and stops there.
  // Invoking back would send an engine value straight back into the engine.
  it('does not invoke the backend', () => {
    const store = useMixerStore();
    store.applyEngineParam({ deck: 'A', slot: 'eq', param: 'low', value: -6 });
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it('ignores an address the store does not hold', () => {
    const store = useMixerStore();
    const before = { ...store.volume };

    store.applyEngineParam({ deck: 'Z', slot: 'fader', param: 'gain', value: 0 });
    store.applyEngineParam({ deck: 'A', slot: 'nope', param: 'gain', value: 0 });
    store.applyEngineParam({ deck: 'A', slot: 'eq', param: 'sub', value: 0 });

    expect({ ...store.volume }).toEqual(before);
    expect(store.eq.A).toEqual({ low: 0, mid: 0, high: 0 });
  });
});
