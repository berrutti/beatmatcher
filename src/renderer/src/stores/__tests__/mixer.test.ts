import { describe, it, expect, vi, beforeEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import { reactive, nextTick } from 'vue';

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

// Reactive, so a store watching it sees a change made after the store exists. A plain
// object would let a mid-set change pass every assertion by triggering nothing.
const settingsMock = reactive({
  pitchRange: 8,
  nudgeSensitivity: 4,
  recordingBitDepth: 24,
  recordingFormat: 'wav',
  recordSession: false,
  filtersEngagedAtStart: false,
  hydrated: true
});

vi.mock('@renderer/stores/settings', () => ({
  DEFAULT_MIXER_ID: 'classic-3band',
  LIVE_MIXER_ID: 'classic-3band-v2',
  useSettingsStore: () => settingsMock
}));

import { useMixerStore, paramKey, FADER_GAIN, FILTER_ACTIVE, type XfaderSide } from '../mixer';
import { editConstants, mixerParams } from '@renderer/utils/sessionCore';
import { LIVE_MIXER_ID } from '@renderer/stores/settings';

const { eqMinDb: EQ_MIN_DB, eqMaxDb: EQ_MAX_DB } = editConstants();
import { invoke } from '@tauri-apps/api/core';

const mockedInvoke = vi.mocked(invoke);

// The mock is module-level, so a test that turns this on would otherwise engage
// filters in every test after it and count as an extra invoke.
beforeEach(() => {
  settingsMock.filtersEngagedAtStart = false;
  settingsMock.hydrated = true;
});

const EQ_LOW = paramKey('eq', 'low');
const EQ_MID = paramKey('eq', 'mid');
const EQ_HIGH = paramKey('eq', 'high');

// Looped from the manifest rather than named, so a param the mixer gains is covered
// without editing this file. That is why the store holds one record over a field each.
const LIVE_SPECS = Object.values(mixerParams(LIVE_MIXER_ID));

describe('setParam', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('holds and invokes every deck-scope address the manifest describes', () => {
    const store = useMixerStore();
    for (const spec of LIVE_SPECS) {
      const key = paramKey(spec.slot, spec.param);
      const midpoint = (spec.min + spec.max) / 2;
      store.setParam('A', key, midpoint);
      expect(store.paramValue('A', key), key).toBe(midpoint);
      expect(mockedInvoke).toHaveBeenCalledWith('set_deck_param', {
        deck: 'A',
        slot: spec.slot,
        param: spec.param,
        value: midpoint
      });
    }
  });

  it('clamps every address to its own descriptor', () => {
    const store = useMixerStore();
    for (const spec of LIVE_SPECS) {
      const key = paramKey(spec.slot, spec.param);
      store.setParam('B', key, spec.max + 100);
      expect(store.paramValue('B', key), key).toBe(spec.max);
      store.setParam('B', key, spec.min - 100);
      expect(store.paramValue('B', key), key).toBe(spec.min);
    }
  });

  it('ignores an address the manifest does not describe', () => {
    const store = useMixerStore();
    store.setParam('A', paramKey('eq', 'sub'), 5);
    expect(store.paramValue('A', paramKey('eq', 'sub'))).toBe(0);
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it('does not affect other decks or params', () => {
    const store = useMixerStore();
    store.setParam('A', EQ_LOW, 4);
    expect(store.paramValue('A', EQ_MID)).toBe(0);
    expect(store.paramValue('A', EQ_HIGH)).toBe(0);
    expect(store.paramValue('B', EQ_LOW)).toBe(0);
  });
});

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

  it('gives the filter and fader sliders their range too', () => {
    const store = useMixerStore();
    expect(store.filterSpec.min).toBe(-1);
    expect(store.filterSpec.max).toBe(1);
    expect(store.faderSpec.min).toBe(0);
    expect(store.faderSpec.max).toBe(1);
  });
});

describe('reset', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('returns every param on every live deck to its descriptor default', () => {
    const store = useMixerStore();
    for (const spec of LIVE_SPECS) {
      const key = paramKey(spec.slot, spec.param);
      for (const deckId of ['A', 'B', 'C', 'D'] as const) store.setParam(deckId, key, spec.max);
    }
    vi.clearAllMocks();

    store.reset();

    for (const spec of LIVE_SPECS) {
      const key = paramKey(spec.slot, spec.param);
      for (const deckId of ['A', 'B', 'C', 'D'] as const) {
        expect(store.paramValue(deckId, key), `${deckId} ${key}`).toBe(spec.defaultValue);
      }
    }
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
      for (const spec of LIVE_SPECS) {
        expect(mockedInvoke).toHaveBeenCalledWith('set_deck_param', {
          deck,
          slot: spec.slot,
          param: spec.param,
          value: spec.defaultValue
        });
      }
      expect(mockedInvoke).toHaveBeenCalledWith('set_cue_active', { deck, active: false });
    }
  });

  it('does not touch deck E', () => {
    const store = useMixerStore();
    store.setParam('E', FADER_GAIN, 0.5);
    vi.clearAllMocks();

    store.reset();

    expect(store.paramValue('E', FADER_GAIN)).toBe(0.5);
    expect(mockedInvoke).not.toHaveBeenCalledWith(
      'set_deck_param',
      expect.objectContaining({ deck: 'E' })
    );
  });
});

describe('filters engaged at start', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    settingsMock.filtersEngagedAtStart = false;
    settingsMock.hydrated = true;
  });

  it('leaves filters off when the preference is off', () => {
    const store = useMixerStore();
    for (const deckId of ['A', 'B', 'C', 'D'] as const) {
      expect(store.paramActive(deckId, FILTER_ACTIVE), deckId).toBe(false);
    }
  });

  it('engages every live deck when the preference is on', () => {
    settingsMock.filtersEngagedAtStart = true;
    const store = useMixerStore();
    for (const deckId of ['A', 'B', 'C', 'D'] as const) {
      expect(store.paramActive(deckId, FILTER_ACTIVE), deckId).toBe(true);
    }
    expect(store.paramActive('E', FILTER_ACTIVE)).toBe(false);
  });

  it('ignores the preference being switched on after launch', async () => {
    const store = useMixerStore();

    settingsMock.filtersEngagedAtStart = true;
    await nextTick();

    expect(store.paramActive('A', FILTER_ACTIVE)).toBe(false);
  });

  it('engages at launch when the settings arrive after the store is created', async () => {
    settingsMock.hydrated = false;
    settingsMock.filtersEngagedAtStart = true;
    const store = useMixerStore();

    settingsMock.hydrated = true;
    await nextTick();

    expect(store.paramActive('A', FILTER_ACTIVE)).toBe(true);
  });

  it('re-engages after a reset, which would otherwise restore the descriptor default', () => {
    settingsMock.filtersEngagedAtStart = true;
    const store = useMixerStore();

    store.reset();

    expect(store.paramActive('A', FILTER_ACTIVE)).toBe(true);
  });
});

describe('applyEngineParam', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('mirrors every deck param the mixer holds', () => {
    const store = useMixerStore();

    for (const spec of LIVE_SPECS) {
      const key = paramKey(spec.slot, spec.param);
      store.applyEngineParam({ deck: 'A', slot: spec.slot, param: spec.param, value: spec.max });
      expect(store.paramValue('A', key), key).toBe(spec.max);
    }
  });

  it('does not invoke the backend', () => {
    const store = useMixerStore();
    store.applyEngineParam({ deck: 'A', slot: 'eq', param: 'low', value: -6 });
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it('ignores an address the store does not hold', () => {
    const store = useMixerStore();
    store.setParam('A', FADER_GAIN, 0.7);
    const before = { ...store.params.A };

    store.applyEngineParam({ deck: 'Z', slot: 'fader', param: 'gain', value: 0 });
    store.applyEngineParam({ deck: 'A', slot: 'nope', param: 'gain', value: 0 });
    store.applyEngineParam({ deck: 'A', slot: 'eq', param: 'sub', value: 0 });

    expect({ ...store.params.A }).toEqual(before);
  });
});

describe('crossfader', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('starts centred with every deck through', () => {
    const store = useMixerStore();

    expect(store.xfaderPosition).toBe(0);
    for (const deckId of ['A', 'B', 'C', 'D'] as const) {
      expect(store.xfaderAssign[deckId]).toBe('thru');
    }
  });

  it('clamps the position to the throw', () => {
    const store = useMixerStore();

    store.setXfaderPosition(-4);
    expect(store.xfaderPosition).toBe(-1);
    store.setXfaderPosition(4);
    expect(store.xfaderPosition).toBe(1);
  });

  it('sends the position and the assign to Rust', () => {
    const store = useMixerStore();

    store.setXfaderPosition(0.5);
    expect(mockedInvoke).toHaveBeenCalledWith('set_xfader_position', { position: 0.5 });

    store.setXfaderAssign('B', 'a');
    expect(mockedInvoke).toHaveBeenCalledWith('set_xfader_assign', { deck: 'B', assign: 'a' });
  });

  it('mirrors an engine-driven position that carries no deck', () => {
    const store = useMixerStore();

    store.applyEngineParam({ deck: '', slot: 'xfader', param: 'position', value: -0.75 });

    expect(store.xfaderPosition).toBe(-0.75);
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it('mirrors an engine-driven assign, which travels by name', () => {
    const store = useMixerStore();

    store.applyEngineAssign({ deck: 'C', assign: 'b' });
    expect(store.xfaderAssign.C).toBe('b');

    store.applyEngineAssign({ deck: 'C', assign: 'thru' });
    expect(store.xfaderAssign.C).toBe('thru');
  });

  it('moves the deck between sides and clears it when the lit side is pressed again', () => {
    const store = useMixerStore();

    store.toggleXfaderAssign('A', 'a');
    expect(store.xfaderAssign.A).toBe('a');

    store.toggleXfaderAssign('A', 'b');
    expect(store.xfaderAssign.A).toBe('b');

    store.toggleXfaderAssign('A', 'b');
    expect(store.xfaderAssign.A).toBe('thru');
    expect(mockedInvoke).toHaveBeenLastCalledWith('set_xfader_assign', {
      deck: 'A',
      assign: 'thru'
    });
  });

  it('never lights both sides, whatever the press order', () => {
    const store = useMixerStore();
    const sides: XfaderSide[] = ['a', 'b'];

    for (const first of sides) {
      for (const second of sides) {
        for (const third of sides) {
          store.setXfaderAssign('A', 'thru');
          for (const side of [first, second, third]) {
            store.toggleXfaderAssign('A', side);
            const lit = sides.filter((candidate) => store.xfaderAssign.A === candidate);
            expect(lit.length).toBeLessThanOrEqual(1);
          }
        }
      }
    }
  });

  it('returns to centre and through on reset', () => {
    const store = useMixerStore();
    store.setXfaderPosition(1);
    store.setXfaderAssign('A', 'b');

    store.reset();

    expect(store.xfaderPosition).toBe(0);
    expect(store.xfaderAssign.A).toBe('thru');
  });
});

describe('scrub mute', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('restores each deck to its own volume when two scrubs overlap', () => {
    const store = useMixerStore();
    store.setParam('A', FADER_GAIN, 0.8);
    store.setParam('B', FADER_GAIN, 0.3);

    store.startScrubMute('A');
    store.startScrubMute('B');
    expect(store.paramValue('A', FADER_GAIN)).toBe(0);
    expect(store.paramValue('B', FADER_GAIN)).toBe(0);

    store.endScrubMute('A');
    store.endScrubMute('B');

    expect(store.paramValue('A', FADER_GAIN)).toBe(0.8);
    expect(store.paramValue('B', FADER_GAIN)).toBe(0.3);
  });

  it('keeps the first saved volume when a scrub end is lost and another starts', () => {
    const store = useMixerStore();
    store.setParam('A', FADER_GAIN, 0.6);

    store.startScrubMute('A');
    store.startScrubMute('A');
    store.endScrubMute('A');

    expect(store.paramValue('A', FADER_GAIN)).toBe(0.6);
  });

  it('does nothing on an end without a start', () => {
    const store = useMixerStore();
    store.setParam('C', FADER_GAIN, 0.5);

    store.endScrubMute('C');

    expect(store.paramValue('C', FADER_GAIN)).toBe(0.5);
  });
});
