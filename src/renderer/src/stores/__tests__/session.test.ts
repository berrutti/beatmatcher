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

import {
  useSessionStore,
  SESSION_LOAD_PHASE_KEYS,
  sessionLoadIsMeasured,
  type SessionLoadPhase
} from '../session';
import en from '@renderer/locales/en.json';
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
      { elapsed_ms: 1000, type: 'set_param', deck: 'A', slot: 'fader', param: 'gain', value: 0.8 },
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

    // The library moved to /lib. B.mp3 sits in a subfolder.
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

describe('edits rejected by session-core', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    fakeFs = { '/music/a.mp3': 100 };
    fakeScan = {};
    fakeBms = {};
    dialogResult = null;
    installInvokeMock();
  });

  it('a rejected gesture leaves the session, the undo stack and the redo stack untouched', async () => {
    const store = useSessionStore();
    const editStore = useSessionEditStore();
    fakeBms['/sessions/mix.bms'] = sessionContent('/music/a.mp3');
    await store.openSessionFromPath('/sessions/mix.bms');

    await editStore.commitGesture(
      'A',
      'gain',
      [
        { ms: 1000, value: 0.5 },
        { ms: 3000, value: 0.9 }
      ],
      1000,
      3000
    );
    expect(editStore.canUndo).toBe(true);

    editStore.undo();
    expect(editStore.canUndo).toBe(false);
    expect(editStore.canRedo).toBe(true);

    const before = store.session?.events;
    // Shorter than MIN_GESTURE_MS, so session-core returns its input.
    await editStore.commitGesture('A', 'gain', [{ ms: 1000, value: 0.5 }], 1000, 1010);

    expect(store.session?.events).toBe(before);
    expect(editStore.canUndo).toBe(false);
    expect(editStore.canRedo).toBe(true);
    expect(editStore.dirty).toBe(false);
  });
});

describe('playback gate while a session is decoding', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  function loading(store: ReturnType<typeof useSessionStore>, done: boolean, bytes = 0) {
    store.loadProgress = {
      path: '/s.bms',
      phase: 'decoding',
      loadedBytes: bytes,
      totalBytes: 100,
      loadedTracks: done ? 4 : 1,
      totalTracks: 4,
      done
    };
  }

  it('reports loading until the backend says every track is decoded', () => {
    const store = useSessionStore();
    expect(store.isLoading).toBe(false);
    loading(store, false);
    expect(store.isLoading).toBe(true);
    loading(store, true);
    expect(store.isLoading).toBe(false);
  });

  it('refuses to start playback while still decoding', async () => {
    const store = useSessionStore();
    loading(store, false);
    await store.play(0);

    expect(store.isPlaying).toBe(false);
    expect(vi.mocked(invoke).mock.calls.some((c) => c[0] === 'start_session_playback')).toBe(false);
  });

  it('weights progress by bytes, not by track count', () => {
    const store = useSessionStore();
    loading(store, false, 30);
    expect(store.loadedFraction).toBeCloseTo(0.3, 6);
  });

  it('falls back to track count when no byte total is known', () => {
    const store = useSessionStore();
    store.loadProgress = {
      path: '/s.bms',
      phase: 'decoding',
      loadedBytes: 0,
      totalBytes: 0,
      loadedTracks: 1,
      totalTracks: 4,
      done: false
    };
    expect(store.loadedFraction).toBeCloseTo(0.25, 6);
  });

  it('reads as fully loaded when nothing is being tracked', () => {
    const store = useSessionStore();
    expect(store.loadedFraction).toBe(1);
    expect(store.isLoading).toBe(false);
  });
});

function pathArg(args: unknown): string {
  if (args && typeof args === 'object' && 'path' in args && typeof args.path === 'string') {
    return args.path;
  }
  return '';
}

describe('a preload failure against the session it was started for', () => {
  let preloadRejects: Record<string, () => void>;

  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    fakeBms = {};
    preloadRejects = {};
    mockedInvoke.mockImplementation((cmd, args) => {
      const path = pathArg(args);
      if (cmd === 'read_file') return Promise.resolve(fakeBms[path] ?? null);
      if (cmd === 'preload_session') {
        return new Promise((_resolve, reject) => {
          preloadRejects[path] = () => reject(new Error('decode failed'));
        });
      }
      return Promise.resolve(null);
    });
  });

  it('leaves the newer session loading when the older one fails to preload', async () => {
    const store = useSessionStore();
    fakeBms['/sessions/a.bms'] = sessionContent('/music/a.mp3');
    fakeBms['/sessions/b.bms'] = sessionContent('/music/b.mp3');

    await store.openSessionFromPath('/sessions/a.bms');
    await store.openSessionFromPath('/sessions/b.bms');

    preloadRejects['/sessions/a.bms']();
    await vi.waitFor(() => {
      expect(preloadRejects['/sessions/b.bms']).toBeTypeOf('function');
    });
    await Promise.resolve();
    await Promise.resolve();

    expect(store.loadProgress?.path).toBe('/sessions/b.bms');
    expect(store.isLoading).toBe(true);
  });

  it('clears the progress when the session that failed is still the loaded one', async () => {
    const store = useSessionStore();
    fakeBms['/sessions/a.bms'] = sessionContent('/music/a.mp3');

    await store.openSessionFromPath('/sessions/a.bms');
    preloadRejects['/sessions/a.bms']();
    await vi.waitFor(() => {
      expect(store.loadProgress).toBe(null);
    });

    expect(store.isLoading).toBe(false);
  });
});

describe('session load phases', () => {
  const PHASES: SessionLoadPhase[] = ['reading', 'parsing', 'decoding', 'indexing', 'done'];

  it('names every phase with a key the locale actually defines', () => {
    for (const phase of PHASES) {
      const key = SESSION_LOAD_PHASE_KEYS[phase];
      expect(key, phase).toBeDefined();
      const leaf = key.replace('session.', '');
      expect(en.session[leaf as keyof typeof en.session], key).toBeTruthy();
    }
  });

  it('measures only the phases that report increments', () => {
    expect(sessionLoadIsMeasured('decoding')).toBe(true);
    expect(sessionLoadIsMeasured('done')).toBe(true);
    for (const phase of ['reading', 'parsing', 'indexing'] as const) {
      expect(sessionLoadIsMeasured(phase), phase).toBe(false);
    }
  });
});
