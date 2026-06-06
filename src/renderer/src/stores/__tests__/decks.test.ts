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

import { useDecksStore } from '../decks';
import { invoke } from '@tauri-apps/api/core';

const mockedInvoke = vi.mocked(invoke);

describe('requestEditMode', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('returns false and does not enter when decks are active', async () => {
    const store = useDecksStore();
    store.deckA.loopPlaying = true;

    const result = await store.requestEditMode();

    expect(result).toBe(false);
    expect(store.editMode).toBe(false);
    expect(mockedInvoke).not.toHaveBeenCalledWith('stop', expect.anything());
  });

  it('enters and returns true when no decks are active', async () => {
    const store = useDecksStore();

    const result = await store.requestEditMode();

    expect(result).toBe(true);
    expect(store.editMode).toBe(true);
    expect(mockedInvoke).not.toHaveBeenCalledWith('stop', expect.anything());
  });

  it('force-stops all playing live decks then enters', async () => {
    const store = useDecksStore();
    store.deckA.loopPlaying = true;
    store.deckC.loopPlaying = true;

    const result = await store.requestEditMode(true);

    expect(result).toBe(true);
    expect(mockedInvoke).toHaveBeenCalledWith('stop', { deck: 'A' });
    expect(mockedInvoke).toHaveBeenCalledWith('stop', { deck: 'C' });
    expect(mockedInvoke).not.toHaveBeenCalledWith('stop', { deck: 'B' });
    expect(mockedInvoke).not.toHaveBeenCalledWith('stop', { deck: 'D' });
    expect(store.editMode).toBe(true);
    expect(store.deckA.loopPlaying).toBe(false);
    expect(store.deckC.loopPlaying).toBe(false);
  });

  it('does not stop deck E when force-entering', async () => {
    const store = useDecksStore();
    store.deckE.loopPlaying = true;

    await store.requestEditMode(true);

    expect(mockedInvoke).not.toHaveBeenCalledWith('stop', { deck: 'E' });
    expect(store.deckE.loopPlaying).toBe(true);
    expect(store.editMode).toBe(true);
  });
});
