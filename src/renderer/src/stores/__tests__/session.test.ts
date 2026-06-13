import { describe, it, expect, vi, beforeEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(null)
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {})
}));

vi.mock('@tauri-apps/plugin-store', () => ({
  load: vi.fn().mockResolvedValue({})
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn()
}));

vi.mock('@renderer/stores/settings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@renderer/stores/settings')>();
  return {
    ...actual,
    useSettingsStore: () => ({
      nudgeSensitivity: 4
    })
  };
});

import { useSessionStore } from '../session';
import { useSessionEditStore } from '../sessionEdit';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

const mockedInvoke = vi.mocked(invoke);
const mockedOpen = vi.mocked(open);

// Paths that "exist" on the fake filesystem, with their sizes.
let fakeFs: Record<string, number>;
// Audio files found under a folder by the mocked scan_folder, keyed by folder.
let fakeScan: Record<string, string[]>;
// What the native folder picker returns.
let dialogResult: string | null;

// Content served by the mocked read_file command, keyed by path.
let fakeBms: Record<string, string>;

function installInvokeMock() {
  mockedInvoke.mockImplementation((cmd, args) => {
    if (cmd === 'files_info') {
      const { paths } = args as { paths: string[] };
      return Promise.resolve(paths.map((p) => fakeFs[p] ?? null));
    }
    if (cmd === 'scan_folder') {
      const { path } = args as { path: string };
      return Promise.resolve(fakeScan[path] ?? []);
    }
    if (cmd === 'read_file') {
      const { path } = args as { path: string };
      return Promise.resolve(fakeBms[path] ?? null);
    }
    return Promise.resolve(null);
  });
  mockedOpen.mockImplementation(async () => dialogResult);
}

function sessionContent(...trackPaths: string[]): string {
  const decks = ['A', 'B', 'C', 'D'];
  return JSON.stringify({
    version: 1,
    startedAt: '2026-06-11T00:00:00Z',
    events: [
      ...trackPaths.map((path, i) => ({
        elapsed_ms: 0,
        type: 'deck_snapshot',
        deck: decks[i],
        path,
        is_playing: true,
        position_sec: 0
      })),
      { elapsed_ms: 1000, type: 'set_volume', deck: 'A', gain: 0.8 },
      { elapsed_ms: 5000, type: 'stop', deck: 'A' }
    ]
  });
}

describe('missing track detection and relocation', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    fakeFs = {};
    fakeScan = {};
    fakeBms = {};
    dialogResult = null;
    installInvokeMock();
  });

  it('flags tracks whose files do not exist after load', async () => {
    const store = useSessionStore();
    fakeBms['/sessions/mix.bms'] = sessionContent('/music/a.mp3');
    await store.openSessionFromPath('/sessions/mix.bms');
    await vi.waitFor(() => {
      expect(store.missingTracks).toEqual(['/music/a.mp3']);
    });
  });

  it('clears the flag when the file exists', async () => {
    fakeFs['/music/a.mp3'] = 100;
    const store = useSessionStore();
    fakeBms['/sessions/mix.bms'] = sessionContent('/music/a.mp3');
    await store.openSessionFromPath('/sessions/mix.bms');
    await vi.waitFor(() => {
      expect(store.missingTracks).toEqual([]);
    });
  });

  it('locateMissingTracks resolves every missing file found under the picked folder', async () => {
    const store = useSessionStore();
    const editStore = useSessionEditStore();
    fakeBms['/sessions/mix.bms'] = sessionContent('/music/a.mp3', '/music/b.mp3');
    await store.openSessionFromPath('/sessions/mix.bms');
    await vi.waitFor(() => {
      expect(store.missingTracks).toEqual(['/music/a.mp3', '/music/b.mp3']);
    });

    // The library moved to /lib; b.mp3 sits in a subfolder.
    dialogResult = '/lib';
    fakeScan['/lib'] = ['/lib/a.mp3', '/lib/deep/b.mp3'];
    fakeFs['/lib/a.mp3'] = 100;
    fakeFs['/lib/deep/b.mp3'] = 100;
    await editStore.locateMissingTracks();

    expect(store.session?.events[0].path).toBe('/lib/a.mp3');
    expect(store.session?.events[1].path).toBe('/lib/deep/b.mp3');
    await vi.waitFor(() => {
      expect(store.missingTracks).toEqual([]);
    });
    expect(editStore.dirty).toBe(true);
  });

  it('leaves files alone that are not found under the picked folder', async () => {
    const store = useSessionStore();
    const editStore = useSessionEditStore();
    fakeBms['/sessions/mix.bms'] = sessionContent('/music/a.mp3', '/music/b.mp3');
    await store.openSessionFromPath('/sessions/mix.bms');
    await vi.waitFor(() => {
      expect(store.missingTracks).toEqual(['/music/a.mp3', '/music/b.mp3']);
    });

    dialogResult = '/lib';
    fakeScan['/lib'] = ['/lib/a.mp3'];
    fakeFs['/lib/a.mp3'] = 100;
    await editStore.locateMissingTracks();

    expect(store.session?.events[0].path).toBe('/lib/a.mp3');
    expect(store.session?.events[1].path).toBe('/music/b.mp3');
    await vi.waitFor(() => {
      expect(store.missingTracks).toEqual(['/music/b.mp3']);
    });
  });

  // The file came back at its original path (folder renamed back, drive
  // reconnected) and the user picks the folder the session already points
  // into. Nothing in the event list changes, so the indicator must be cleared
  // by an explicit recheck, not by reacting to an event edit.
  it('clears the indicator when the located file is at its original path', async () => {
    const store = useSessionStore();
    const editStore = useSessionEditStore();
    fakeBms['/sessions/mix.bms'] = sessionContent('/music/a.mp3');
    await store.openSessionFromPath('/sessions/mix.bms');
    await vi.waitFor(() => {
      expect(store.missingTracks).toEqual(['/music/a.mp3']);
    });

    fakeFs['/music/a.mp3'] = 100;
    dialogResult = '/music';
    fakeScan['/music'] = ['/music/a.mp3'];
    await editStore.locateMissingTracks();

    await vi.waitFor(() => {
      expect(store.missingTracks).toEqual([]);
    });
    // The events did not change, so this must not count as an edit.
    expect(editStore.dirty).toBe(false);
  });

  it('does nothing when the folder dialog is cancelled', async () => {
    const store = useSessionStore();
    const editStore = useSessionEditStore();
    fakeBms['/sessions/mix.bms'] = sessionContent('/music/a.mp3');
    await store.openSessionFromPath('/sessions/mix.bms');
    await vi.waitFor(() => {
      expect(store.missingTracks).toEqual(['/music/a.mp3']);
    });

    dialogResult = null;
    await editStore.locateMissingTracks();

    expect(store.missingTracks).toEqual(['/music/a.mp3']);
    expect(editStore.dirty).toBe(false);
  });
});
