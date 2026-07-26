import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import { mount } from '@vue/test-utils';

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
  STORAGE_KEYS: { deckCount: 'deckCount', showWaveformStrip: 'showWaveformStrip' }
}));

vi.mock('@renderer/stores/settings', () => ({
  DEFAULT_MIXER_ID: 'classic-3band',
  LIVE_MIXER_ID: 'classic-3band-v2',
  useSettingsStore: () => ({
    pitchRange: 8,
    nudgeSensitivity: 4,
    recordingBitDepth: 24,
    recordingFormat: 'wav',
    recordSession: false,
    deckAccents: {},
    setDeckAccents: vi.fn()
  })
}));

import Mixer from '@renderer/components/mixer/Mixer.vue';
import { useMixerStore } from '@renderer/stores/mixer';
import { DECKS_DISPOSITION, type DeckId } from '@renderer/stores/decks';

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

const DECKS: DeckId[] = [...DECKS_DISPOSITION];

describe('the mixer under fuzzed swarm drags', () => {
  let wrapper: ReturnType<typeof mount> | null = null;

  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    vi.useFakeTimers();
    wrapper = mount(Mixer, { global: { mocks: { $t: (key: string) => key } } });
  });

  afterEach(() => {
    wrapper?.unmount();
    wrapper = null;
    vi.useRealTimers();
  });

  // The component computes each drag as a delta against the dragged deck and applies
  // it to every selected channel; only the store clamps. Driven through the real
  // slider so the component's own arithmetic runs.
  function dragFader(index: number, value: number) {
    const faders = wrapper?.findAll('input.mixer__fader') ?? [];
    const fader = faders[index];
    if (!fader) throw new Error(`no fader ${index} of ${faders.length}`);
    const element = fader.element;
    if (!(element instanceof HTMLInputElement)) throw new Error('fader is not an input');
    element.value = String(value);
    fader.trigger('input');
  }

  it('mounts one fader per live deck', () => {
    expect(wrapper?.findAll('input.mixer__fader').length).toBe(DECKS.length);
  });

  it('keeps every channel in range however the group is dragged', async () => {
    const store = useMixerStore();
    const random = makeRandom(5);
    store.setSwarmMode(true);
    for (const deck of DECKS) store.setSwarmChannel(deck, random() < 0.6);

    for (let step = 0; step < 1500; step++) {
      dragFader(Math.floor(random() * DECKS.length), random());
      await Promise.resolve();

      for (const deck of DECKS) {
        expect(Number.isFinite(store.volume[deck]), `step ${step}`).toBe(true);
        expect(store.volume[deck], `step ${step}`).toBeGreaterThanOrEqual(0);
        expect(store.volume[deck], `step ${step}`).toBeLessThanOrEqual(1);
      }
    }
  });

  it('moves the dragged channel to exactly where it was dropped', async () => {
    const store = useMixerStore();
    const random = makeRandom(9);
    store.setSwarmMode(true);
    for (const deck of DECKS) store.setSwarmChannel(deck, true);

    for (let step = 0; step < 500; step++) {
      const index = Math.floor(random() * DECKS.length);
      // Mid-range so the target is never itself clamped, which is what makes this
      // an assertion about the delta rather than about the clamp.
      const target = 0.3 + random() * 0.4;

      dragFader(index, target);
      await Promise.resolve();

      expect(store.volume[DECKS[index]], `step ${step}`).toBeCloseTo(target, 10);
    }
  });

  it('leaves unselected channels alone in swarm mode', async () => {
    const store = useMixerStore();
    store.setSwarmMode(true);
    store.setSwarmChannel(DECKS[0], true);
    for (const deck of DECKS.slice(1)) store.setSwarmChannel(deck, false);
    const untouched = DECKS[2];
    store.setVolume(untouched, 0.42);

    dragFader(0, 0.1);
    await Promise.resolve();
    dragFader(0, 0.9);
    await Promise.resolve();

    expect(store.volume[untouched]).toBe(0.42);
  });

  it('moves only the dragged channel with swarm off', async () => {
    const store = useMixerStore();
    const random = makeRandom(13);
    for (const deck of DECKS) store.setVolume(deck, 0.5);

    for (let step = 0; step < 300; step++) {
      const index = Math.floor(random() * DECKS.length);
      const target = random();
      dragFader(index, target);
      await Promise.resolve();

      expect(store.volume[DECKS[index]], `step ${step}`).toBeCloseTo(target, 10);
      for (const [other, deck] of DECKS.entries()) {
        if (other !== index) expect(store.volume[deck], `step ${step}`).toBe(0.5);
      }
      store.setVolume(DECKS[index], 0.5);
    }
  });
});
