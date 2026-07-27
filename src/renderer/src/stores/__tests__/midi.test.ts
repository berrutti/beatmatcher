import { describe, it, expect, vi, beforeEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';

// The store enumerates ports as soon as it is created, so the mock has to answer
// that with a list rather than null.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (command: string) => (command === 'list_midi_inputs' ? [] : null))
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn()
}));

vi.mock('@renderer/utils/storage', () => ({
  storageGet: vi.fn().mockReturnValue(null),
  storageSet: vi.fn(),
  STORAGE_KEYS: { midiInput: 'midiInput' }
}));

import { useMidiStore } from '../midi';
import { listen } from '@tauri-apps/api/event';

const mockedListen = vi.mocked(listen);

describe('the MIDI monitor', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  // Settings mounting twice before the first registration resolves used to leave a
  // leaked listener behind, and every message was counted twice.
  it('registers one listener when two starts overlap', async () => {
    const unlisten = vi.fn();
    mockedListen.mockImplementation(
      () => new Promise((resolve) => setTimeout(() => resolve(unlisten), 0))
    );
    const store = useMidiStore();

    await Promise.all([store.startMonitor(), store.startMonitor()]);

    expect(mockedListen).toHaveBeenCalledTimes(1);
  });

  it('unregisters on stop and registers again on the next start', async () => {
    const unlisten = vi.fn();
    mockedListen.mockResolvedValue(unlisten);
    const store = useMidiStore();

    await store.startMonitor();
    await store.stopMonitor();
    expect(unlisten).toHaveBeenCalledTimes(1);

    await store.startMonitor();
    expect(mockedListen).toHaveBeenCalledTimes(2);
  });
});
