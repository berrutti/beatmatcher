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

import { useDecksStore, type LoadableTrack } from '../decks';
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
      loopRegionCleared: false,
      loopRegion: null
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
      loopRegionCleared: true,
      loopRegion: null
    });

    expect(decks.deckB.loopRegion).toBeNull();
  });

  it('draws the region a controller press defined', () => {
    const decks = useDecksStore();

    decks.deckA.applyEngineTransport({
      isPlaying: true,
      isCueing: false,
      cuePointSec: 10,
      positionSec: 11,
      loopActive: true,
      loopRegionCleared: false,
      loopRegion: { startSec: 10, endSec: 14, beats: 8 }
    });

    expect(decks.deckA.loopRegion).toEqual({ startSec: 10, endSec: 14, beats: 8 });
  });

  it('leaves a cached region alone when the payload carries none', () => {
    const decks = useDecksStore();
    const region = { startSec: 1, endSec: 2, beats: 4 };
    decks.deckB.loopRegion = region;

    decks.deckB.applyEngineTransport({
      isPlaying: true,
      isCueing: false,
      cuePointSec: 1,
      positionSec: 1.5,
      loopActive: false,
      loopRegionCleared: false,
      loopRegion: null
    });

    expect(decks.deckB.loopRegion).toEqual(region);
  });
});

describe('a displayed bpm is exactly reachable', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('sets 129.05 with no rounding drift', () => {
    const decks = useDecksStore();
    decks.deckA.setTrackBpm(120);

    decks.deckA.setTargetBpm(129.05);

    expect(decks.deckA.targetBpm).toBe(129.05);
    expect(mockedInvoke).toHaveBeenCalledWith('set_playback_rate', {
      deck: 'A',
      rate: 129.05 / 120
    });
  });
});

describe('an engine rate move on a deck with no beat grid', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('still slows the interpolated playhead', () => {
    const clock = vi.spyOn(performance, 'now');
    clock.mockReturnValue(0);

    const decks = useDecksStore();
    const deck = decks.deckA;
    deck.applyEngineTransport({
      isPlaying: true,
      isCueing: false,
      cuePointSec: 0,
      positionSec: 0,
      loopActive: false,
      loopRegionCleared: false,
      loopRegion: null
    });

    clock.mockReturnValue(1000);
    deck.applyEngineRate(0.5);
    clock.mockReturnValue(2000);

    expect(deck.trackBpm).toBeNull();
    expect(deck.trackPosition).toBeCloseTo(1.5, 6);
    clock.mockRestore();
  });
});

describe('a dropped track names itself before it has loaded', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  const track: LoadableTrack = {
    path: '/music/next.mp3',
    name: 'Next Track',
    bpm: 128,
    silenceEnd: 0,
    beatOffset: 0.5,
    onBeatOffsetChange: () => {}
  };

  it('shows the new name while the decode is still running', async () => {
    let releaseLoad = () => {};
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd !== 'load_track') return {};
      await new Promise<void>((resolve) => {
        releaseLoad = resolve;
      });
      return {};
    });

    const decks = useDecksStore();
    const loading = decks.deckA.loadTrack(track);
    await Promise.resolve();

    expect(decks.deckA.trackName).toBe('Next Track');
    expect(decks.deckA.loading).toBe(true);

    releaseLoad();
    await loading;
    expect(decks.deckA.trackName).toBe('Next Track');
  });

  it('clears what the previous track left behind', async () => {
    vi.mocked(invoke).mockResolvedValue({});
    const decks = useDecksStore();
    await decks.deckA.loadTrack({ ...track, name: 'First' });
    expect(decks.deckA.trackName).toBe('First');

    let releaseLoad = () => {};
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd !== 'load_track') return {};
      await new Promise<void>((resolve) => {
        releaseLoad = resolve;
      });
      return {};
    });
    const loading = decks.deckA.loadTrack({ ...track, name: 'Second' });
    await Promise.resolve();

    expect(decks.deckA.trackName).toBe('Second');
    expect(decks.deckA.coverArt, 'the old art must not sit under the new name').toBe(null);

    releaseLoad();
    await loading;
  });
});

describe('a deck that is still loading says so', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  const track: LoadableTrack = {
    path: '/music/next.mp3',
    name: 'Next Track',
    bpm: 128,
    silenceEnd: 0,
    beatOffset: 0.5,
    onBeatOffsetChange: () => {}
  };

  it('is not loaded until the decode returns, even though it is named', async () => {
    let releaseLoad = () => {};
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd !== 'load_track') return {};
      await new Promise<void>((resolve) => {
        releaseLoad = resolve;
      });
      return {};
    });

    const decks = useDecksStore();
    const loading = decks.deckA.loadTrack(track);
    await Promise.resolve();

    // The name is there so a glance reads the right track, but the deck holds
    // nothing yet, which is what the UI dims until the decode lands.
    expect(decks.deckA.trackName).toBe('Next Track');
    expect(decks.deckA.trackLoaded).toBe(false);
    expect(decks.deckA.loading).toBe(true);

    releaseLoad();
    await loading;
    expect(decks.deckA.trackLoaded).toBe(true);
    expect(decks.deckA.loading).toBe(false);
  });
});

describe('swapping a track shows the loading state too', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  const track: LoadableTrack = {
    path: '/music/next.mp3',
    name: 'Next Track',
    bpm: 128,
    silenceEnd: 0,
    beatOffset: 0.5,
    onBeatOffsetChange: () => {}
  };

  it('reports nothing loaded while a deck that held a track decodes the next one', async () => {
    vi.mocked(invoke).mockResolvedValue({});
    const decks = useDecksStore();
    await decks.deckA.loadTrack({ ...track, name: 'First' });
    expect(decks.deckA.trackLoaded).toBe(true);

    let releaseLoad = () => {};
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd !== 'load_track') return {};
      await new Promise<void>((resolve) => {
        releaseLoad = resolve;
      });
      return {};
    });
    const loading = decks.deckA.loadTrack({ ...track, name: 'Second' });
    await Promise.resolve();

    expect(decks.deckA.trackLoaded).toBe(false);
    expect(decks.deckA.trackName).toBe('Second');

    releaseLoad();
    await loading;
    expect(decks.deckA.trackLoaded).toBe(true);
  });
});

describe('a decode that fails leaves the deck usable', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  const track: LoadableTrack = {
    path: '/music/missing.mp3',
    name: 'Missing Track',
    bpm: 128,
    silenceEnd: 0,
    beatOffset: 0.5,
    onBeatOffsetChange: () => {}
  };

  it('clears the loading gate when load_track rejects', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'load_track') throw new Error('no such file');
      return {};
    });

    const decks = useDecksStore();
    await expect(decks.deckA.loadTrack(track)).rejects.toThrow('no such file');

    // Otherwise `loading` stays true and `acceptsCommands` refuses every press
    // for the rest of the session.
    expect(decks.deckA.loading).toBe(false);
    expect(decks.deckA.acceptsCommands).toBe(true);
  });

  it('does not leave the name of a track it never loaded', async () => {
    vi.mocked(invoke).mockResolvedValue({});
    const decks = useDecksStore();
    await decks.deckA.loadTrack({ ...track, name: 'First', path: '/music/first.mp3' });

    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'load_track') throw new Error('no such file');
      return {};
    });
    await expect(decks.deckA.loadTrack(track)).rejects.toThrow();

    expect(decks.deckA.trackName).toBe('');
  });
});
