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

vi.mock('@renderer/stores/settings', () => ({
  useSettingsStore: () => ({
    pitchRange: 8,
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
import { useAppModeStore } from '../appMode';
import { invoke } from '@tauri-apps/api/core';

const mockedInvoke = vi.mocked(invoke);

describe('switchTo edit', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('does not stop decks when none are playing', async () => {
    const appMode = useAppModeStore();

    await appMode.switchTo('edit');

    expect(appMode.mode).toBe('edit');
    expect(mockedInvoke).not.toHaveBeenCalledWith('stop', expect.anything());
  });

  it('stops all playing live decks when entering edit', async () => {
    const decks = useDecksStore();
    const appMode = useAppModeStore();
    decks.deckA.loopPlaying = true;
    decks.deckC.loopPlaying = true;

    await appMode.switchTo('edit');

    expect(appMode.mode).toBe('edit');
    expect(mockedInvoke).toHaveBeenCalledWith('stop', { deck: 'A' });
    expect(mockedInvoke).toHaveBeenCalledWith('stop', { deck: 'C' });
    expect(mockedInvoke).not.toHaveBeenCalledWith('stop', { deck: 'B' });
    expect(mockedInvoke).not.toHaveBeenCalledWith('stop', { deck: 'D' });
    expect(decks.deckA.loopPlaying).toBe(false);
    expect(decks.deckC.loopPlaying).toBe(false);
  });

  it('does not stop deck E when entering edit', async () => {
    const decks = useDecksStore();
    const appMode = useAppModeStore();
    decks.deckE.loopPlaying = true;

    await appMode.switchTo('edit');

    expect(mockedInvoke).not.toHaveBeenCalledWith('stop', { deck: 'E' });
    expect(decks.deckE.loopPlaying).toBe(true);
    expect(appMode.mode).toBe('edit');
  });
});

// Rust gates MIDI input on its mirror of the mode, so a switch that does not
// reach it leaves a controller live over the session scheduler.
describe('switchTo mirrors the mode to Rust', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('reports every mode it switches to', async () => {
    const appMode = useAppModeStore();

    await appMode.switchTo('session');
    expect(mockedInvoke).toHaveBeenCalledWith('set_app_mode', { mode: 'session' });

    await appMode.switchTo('performance');
    expect(mockedInvoke).toHaveBeenCalledWith('set_app_mode', { mode: 'performance' });
  });

  it('reports before tearing anything down, so entering session stops MIDI first', async () => {
    const appMode = useAppModeStore();

    await appMode.switchTo('session');

    const names = mockedInvoke.mock.calls.map(([command]) => command);
    expect(names[0]).toBe('set_app_mode');
  });

  it('says nothing when the mode does not change', async () => {
    const appMode = useAppModeStore();

    await appMode.switchTo('performance');

    expect(mockedInvoke).not.toHaveBeenCalledWith('set_app_mode', expect.anything());
  });
});

// The push channel is the only way an engine-originated transport move reaches
// the UI, because a MIDI press never returns through an invoke.
describe('applyEngineTransport', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('mirrors the pushed state without invoking back', () => {
    const decks = useDecksStore();

    decks.deckA.applyEngineTransport({
      isPlaying: true,
      isCueing: false,
      cuePointSec: 12.5,
      positionSec: 20,
      loopActive: true,
      loopRegionCleared: false
    });

    expect(decks.deckA.loopPlaying).toBe(true);
    expect(decks.deckA.cuePoint).toBe(12.5);
    expect(decks.deckA.loopActive).toBe(true);
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it('drops the cached loop region when the press destroyed it', () => {
    const decks = useDecksStore();
    decks.deckB.loopRegion = { startSec: 1, endSec: 2, beats: 4 };

    decks.deckB.applyEngineTransport({
      isPlaying: false,
      isCueing: false,
      cuePointSec: 1,
      positionSec: 1,
      loopActive: false,
      loopRegionCleared: true
    });

    expect(decks.deckB.loopRegion).toBeNull();
  });
});
