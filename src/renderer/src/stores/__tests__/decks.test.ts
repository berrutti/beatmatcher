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
  useSettingsStore: () => ({ pitchRange: 8, nudgeSensitivity: 4 })
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
