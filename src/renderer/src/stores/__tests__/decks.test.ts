import { describe, it, expect, vi, beforeEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({}),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock('@tauri-apps/plugin-store', () => ({
  load: vi.fn().mockResolvedValue({}),
}));

vi.mock('@renderer/stores/settings', () => ({
  useSettingsStore: () => ({ pitchRange: 8, nudgeSensitivity: 4 }),
}));

import { useDecksStore } from '../decks';
import { invoke } from '@tauri-apps/api/core';

const mockedInvoke = vi.mocked(invoke);

describe('enterEditMode', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('stops all playing live decks then sets editMode', async () => {
    const store = useDecksStore();
    store.deckA.loopPlaying = true;
    store.deckC.loopPlaying = true;

    await store.enterEditMode();

    expect(mockedInvoke).toHaveBeenCalledWith('stop', { deck: 'A' });
    expect(mockedInvoke).toHaveBeenCalledWith('stop', { deck: 'C' });
    expect(mockedInvoke).not.toHaveBeenCalledWith('stop', { deck: 'B' });
    expect(mockedInvoke).not.toHaveBeenCalledWith('stop', { deck: 'D' });
    expect(store.editMode).toBe(true);
    expect(store.deckA.loopPlaying).toBe(false);
    expect(store.deckC.loopPlaying).toBe(false);
  });

  it('does not stop deck E (edit deck is unaffected)', async () => {
    const store = useDecksStore();
    store.deckE.loopPlaying = true;

    await store.enterEditMode();

    expect(mockedInvoke).not.toHaveBeenCalledWith('stop', { deck: 'E' });
    expect(store.deckE.loopPlaying).toBe(true);
    expect(store.editMode).toBe(true);
  });

  it('enters edit mode when no decks are playing', async () => {
    const store = useDecksStore();

    await store.enterEditMode();

    expect(mockedInvoke).not.toHaveBeenCalledWith('stop', expect.anything());
    expect(store.editMode).toBe(true);
  });
});
