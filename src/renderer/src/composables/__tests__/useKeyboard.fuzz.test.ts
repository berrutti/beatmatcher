// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import { defineComponent, h } from 'vue';
import { mount } from '@vue/test-utils';

// Deck commands read fields off what the backend returns, so the mock answers with
// a payload shaped like `DeckSyncPayload` plus the nudge reply.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({
    isPlaying: false,
    isCueing: false,
    cuePointSec: 0,
    positionSec: 0,
    loopActive: false,
    loopRegionCleared: false,
    loopRegion: null,
    effectiveRate: 1
  })
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
  STORAGE_KEYS: {
    deckCount: 'deckCount',
    collectionHeight: 'collectionHeight',
    savedTracks: 'savedTracks',
    collection: 'collection',
    playlists: 'playlists',
    browserColumns: 'browserColumns',
    bigLibrary: 'bigLibrary',
    showWaveformStrip: 'showWaveformStrip'
  }
}));

import { useKeyboard } from '@renderer/composables/useKeyboard';
import { useMixerStore } from '@renderer/stores/mixer';
import { useAppModeStore } from '@renderer/stores/appMode';
import type { DeckId } from '@renderer/utils/types';

const DIGITS = ['Digit1', 'Digit2', 'Digit3', 'Digit4'] as const;
const DIGIT_DECK: Record<string, DeckId> = {
  Digit1: 'C',
  Digit2: 'A',
  Digit3: 'B',
  Digit4: 'D'
};
const KEYS = [...DIGITS, 'Space', 'KeyQ', 'KeyW', 'ShiftLeft', 'Tab'] as const;

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

const Host = defineComponent({
  setup() {
    useKeyboard();
    return () => h('div');
  }
});

function press(code: string, options: { shift?: boolean; target?: EventTarget } = {}) {
  const event = new KeyboardEvent('keydown', {
    code,
    key: code === 'Space' ? ' ' : code === 'ShiftLeft' ? 'Shift' : code,
    shiftKey: options.shift ?? false,
    bubbles: true,
    cancelable: true
  });
  (options.target ?? window).dispatchEvent(event);
}

function release(code: string, options: { target?: EventTarget } = {}) {
  const event = new KeyboardEvent('keyup', {
    code,
    key: code === 'Space' ? ' ' : code === 'ShiftLeft' ? 'Shift' : code,
    bubbles: true,
    cancelable: true
  });
  (options.target ?? window).dispatchEvent(event);
}

function textInput(): HTMLInputElement {
  const input = document.createElement('input');
  input.type = 'text';
  document.body.appendChild(input);
  return input;
}

describe('keyboard handling under fuzzed key sequences', () => {
  let wrapper: ReturnType<typeof mount> | null = null;

  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    wrapper = mount(Host, { attachTo: document.body });
  });

  afterEach(() => {
    wrapper?.unmount();
    wrapper = null;
    document.body.innerHTML = '';
  });

  function anySelected(mixer: ReturnType<typeof useMixerStore>): boolean {
    return DIGITS.some((code) => mixer.swarmSelected[DIGIT_DECK[code]]);
  }

  it('releases the swarm whenever every held key is released', () => {
    const mixer = useMixerStore();

    for (let seed = 1; seed <= 300; seed++) {
      const random = makeRandom(seed);
      const held = new Set<string>();

      for (let step = 0; step < 12; step++) {
        const code = KEYS[Math.floor(random() * KEYS.length)];
        if (held.has(code) || random() < 0.5) {
          release(code);
          held.delete(code);
        } else {
          press(code, { shift: random() < 0.3 });
          held.add(code);
        }
      }
      for (const code of [...held]) release(code);

      expect(anySelected(mixer), `seed ${seed}`).toBe(false);
      expect(mixer.swarmMode, `seed ${seed}`).toBe(false);
    }
  });

  it('releases the swarm even when focus moved into a text input mid-hold', () => {
    const mixer = useMixerStore();
    const input = textInput();

    for (const code of DIGITS) {
      press(code);
      expect(mixer.swarmSelected[DIGIT_DECK[code]]).toBe(true);
      release(code, { target: input });
      expect(mixer.swarmSelected[DIGIT_DECK[code]]).toBe(false);
    }
    expect(mixer.swarmMode).toBe(false);
  });

  it('routes a digit to cue while space is held and never into the swarm', () => {
    const mixer = useMixerStore();
    const random = makeRandom(77);

    press('Space');
    for (let step = 0; step < 200; step++) {
      const code = DIGITS[Math.floor(random() * DIGITS.length)];
      const deck = DIGIT_DECK[code];
      const before = mixer.cueActive[deck];

      press(code);
      release(code);

      expect(mixer.cueActive[deck], `step ${step}`).toBe(!before);
      expect(mixer.swarmSelected[deck], `step ${step}`).toBe(false);
    }
    release('Space');
    expect(mixer.swarmMode).toBe(false);
  });

  it('ignores digits outside performance mode', async () => {
    const mixer = useMixerStore();
    const appMode = useAppModeStore();
    await appMode.switchTo('edit');
    const random = makeRandom(101);

    for (let step = 0; step < 200; step++) {
      const code = DIGITS[Math.floor(random() * DIGITS.length)];
      press(code, { shift: random() < 0.5 });
      release(code);
    }

    expect(anySelected(mixer)).toBe(false);
    expect(mixer.swarmMode).toBe(false);
    expect(DIGITS.every((code) => mixer.cueActive[DIGIT_DECK[code]] === false)).toBe(true);
  });

  it('ignores keys typed into a text input', () => {
    const mixer = useMixerStore();
    const input = textInput();
    const random = makeRandom(131);

    for (let step = 0; step < 200; step++) {
      const code = KEYS[Math.floor(random() * KEYS.length)];
      press(code, { shift: random() < 0.5, target: input });
      release(code, { target: input });
    }

    expect(anySelected(mixer)).toBe(false);
    expect(mixer.swarmMode).toBe(false);
    expect(DIGITS.every((code) => mixer.cueActive[DIGIT_DECK[code]] === false)).toBe(true);
  });
});
