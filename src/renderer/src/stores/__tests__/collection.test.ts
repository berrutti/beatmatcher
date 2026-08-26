import { describe, it, expect, vi, beforeEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}));

vi.mock('@renderer/utils/storage', () => ({
  // A fresh array/object per call (not mockReturnValue's single shared
  // reference), so collection.ts's savedTracks reactive() wrap - which
  // mutates whatever storageGet returns directly - can't leak state
  // between tests that each set up their own pinia instance.
  storageGet: vi.fn().mockImplementation(() => []),
  storageSet: vi.fn(),
  STORAGE_KEYS: {
    collection: 'collection',
    playlists: 'playlists',
    savedTracks: 'savedTracks',
    bigLibrary: 'bigLibrary',
    metadataOverrides: 'metadataOverrides'
  }
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn()
}));

import { useCollectionStore } from '../collection';
import { invoke, type InvokeArgs } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { storageGet } from '@renderer/utils/storage';

const mockedInvoke = vi.mocked(invoke);
const mockedOpen = vi.mocked(open);
const mockedStorageGet = vi.mocked(storageGet);

// Waits for the async analysis microtasks (files_info + analyze_track) to
// settle so assertions run after the store has processed them.
async function flush() {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

describe('collection store: analyze / reanalyze', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('marks a track ready with a BPM on successful analysis', async () => {
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'files_info') return [1000];
      if (cmd === 'analyze_track') return { bpm: 128, silenceEnd: 0.5 };
      if (cmd === 'read_track_tags') return { title: null, artist: null };
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    const store = useCollectionStore();
    await store.addFilesFromPaths(['/music/track.mp3']);
    await flush();
    const track = store.tracks[0];
    store.analyzeTrack(track.id);
    await flush();

    expect(track.status).toBe('ready');
    expect(store.getBpm(track)).toBe(128);
    expect(track.lastAnalysisFailed).toBe(false);
  });

  it('marks a track ready and loadable when analysis finds no BPM', async () => {
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'files_info') return [1000];
      if (cmd === 'analyze_track') return { bpm: null, silenceEnd: 0.5 };
      if (cmd === 'read_track_tags') return { title: null, artist: null };
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    const store = useCollectionStore();
    await store.addFilesFromPaths(['/music/track.mp3']);
    await flush();
    const track = store.tracks[0];
    store.analyzeTrack(track.id);
    await flush();

    expect(track.status).toBe('ready');
    expect(store.getBpm(track)).toBeNull();
    expect(track.lastAnalysisFailed).toBe(false);
    expect(store.getLoadableTrack('/music/track.mp3')).toMatchObject({
      bpm: null,
      beatOffset: 0.5
    });
  });

  it('treats a detected bpm of zero as no bpm', async () => {
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'files_info') return [1000];
      if (cmd === 'analyze_track') return { bpm: 0, silenceEnd: 0.5 };
      if (cmd === 'read_track_tags') return { title: null, artist: null };
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    const store = useCollectionStore();
    await store.addFilesFromPaths(['/music/track.mp3']);
    await flush();
    const track = store.tracks[0];
    store.analyzeTrack(track.id);
    await flush();

    expect(track.status).toBe('ready');
    expect(store.getBpm(track)).toBeNull();
    expect(store.getSaved('/music/track.mp3')?.bpm).toBeNull();
  });

  it('a track analyzed without a bpm takes a manual one on its detected beat offset', async () => {
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'files_info') return [1000];
      if (cmd === 'analyze_track') return { bpm: null, silenceEnd: 0.5 };
      if (cmd === 'read_track_tags') return { title: null, artist: null };
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    const store = useCollectionStore();
    await store.addFilesFromPaths(['/music/track.mp3']);
    await flush();
    const track = store.tracks[0];
    store.analyzeTrack(track.id);
    await flush();

    store.setBpm(track.id, 130);

    expect(store.getBpm(track)).toBe(130);
    expect(store.getSaved('/music/track.mp3')?.beatOffset).toBe(0.5);
    expect(store.getLoadableTrack('/music/track.mp3')).toMatchObject({ bpm: 130 });
  });

  it('leaves a track whose decode failed unloadable', async () => {
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'files_info') return [1000];
      if (cmd === 'analyze_track') throw new Error('analysis failed');
      if (cmd === 'read_track_tags') return { title: null, artist: null };
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    const store = useCollectionStore();
    await store.addFilesFromPaths(['/music/track.mp3']);
    await flush();
    const track = store.tracks[0];
    store.analyzeTrack(track.id);
    await flush();

    expect(track.status).toBe('error');
    expect(store.getLoadableTrack('/music/track.mp3')).toBeNull();
  });

  it('marks a never-analyzed track as error when analysis fails', async () => {
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'files_info') return [1000];
      if (cmd === 'analyze_track') throw new Error('analysis failed');
      if (cmd === 'read_track_tags') return { title: null, artist: null };
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    const store = useCollectionStore();
    await store.addFilesFromPaths(['/music/track.mp3']);
    await flush();
    const track = store.tracks[0];
    store.analyzeTrack(track.id);
    await flush();

    expect(track.status).toBe('error');
    expect(store.getBpm(track)).toBeNull();
  });

  it('reanalyze: on failure, keeps the previous BPM and status ready instead of discarding it', async () => {
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'files_info') return [1000];
      if (cmd === 'analyze_track') return { bpm: 128, silenceEnd: 0.5 };
      if (cmd === 'read_track_tags') return { title: null, artist: null };
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    const store = useCollectionStore();
    await store.addFilesFromPaths(['/music/track.mp3']);
    await flush();
    const track = store.tracks[0];
    store.analyzeTrack(track.id);
    await flush();
    expect(store.getBpm(track)).toBe(128);

    // Now make the next analysis attempt fail, and reanalyze.
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'analyze_track') throw new Error('analysis failed');
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    store.reanalyzeTrack(track.id);
    await flush();

    expect(track.status).toBe('ready');
    expect(track.lastAnalysisFailed).toBe(true);
    expect(store.getBpm(track)).toBe(128);
  });

  it('reanalyze: on success, updates the BPM and clears lastAnalysisFailed', async () => {
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'files_info') return [1000];
      if (cmd === 'analyze_track') return { bpm: 128, silenceEnd: 0.5 };
      if (cmd === 'read_track_tags') return { title: null, artist: null };
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    const store = useCollectionStore();
    await store.addFilesFromPaths(['/music/track.mp3']);
    await flush();
    const track = store.tracks[0];
    store.analyzeTrack(track.id);
    await flush();

    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'analyze_track') return { bpm: 140, silenceEnd: 0.5 };
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    store.reanalyzeTrack(track.id);
    await flush();

    expect(track.status).toBe('ready');
    expect(track.lastAnalysisFailed).toBe(false);
    expect(store.getBpm(track)).toBe(140);
  });

  it('reanalyze: a failed/low-confidence result does not shift the beat grid used by a later manual Set BPM', async () => {
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'files_info') return [1000];
      if (cmd === 'analyze_track') return { bpm: 128, silenceEnd: 0.5 };
      if (cmd === 'read_track_tags') return { title: null, artist: null };
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    const store = useCollectionStore();
    await store.addFilesFromPaths(['/music/track.mp3']);
    await flush();
    const track = store.tracks[0];
    store.analyzeTrack(track.id);
    await flush();
    expect(store.getSaved('/music/track.mp3')?.beatOffset).toBe(0.5);

    // Reanalysis comes back low-confidence (no usable bpm) but with a
    // different silenceEnd from a fresh detector run.
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'analyze_track') return { bpm: null, silenceEnd: 0.9 };
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    store.reanalyzeTrack(track.id);
    await flush();
    expect(track.status).toBe('ready');
    expect(track.lastAnalysisFailed).toBe(true);
    expect(store.getBpm(track)).toBe(128);

    store.setBpm(track.id, 130);

    expect(store.getSaved('/music/track.mp3')?.beatOffset).toBe(0.5);
  });
});

describe('collection store: relink preserves metadata overrides', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('carries a user-edited metadata override over to the new path when a missing track is relinked', async () => {
    mockedStorageGet.mockImplementation((key: string, fallback: unknown) => {
      if (key === 'collection') {
        return [{ name: 'track.mp3', size: 1000, path: '/old/track.mp3', addedAt: 1000 }];
      }
      return fallback;
    });
    mockedInvoke.mockImplementation(async (cmd: string, args?: InvokeArgs) => {
      if (cmd === 'files_info') {
        const paths = args && 'paths' in args && Array.isArray(args.paths) ? args.paths : [];
        return paths.map((p) => (p === '/old/track.mp3' ? null : 1000));
      }
      if (cmd === 'scan_folder') return ['/new/track.mp3'];
      if (cmd === 'read_track_tags') return { title: null, artist: null };
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    const store = useCollectionStore();
    await flush();
    const track = store.tracks[0];
    expect(track.status).toBe('missing');

    store.setMetadataField(track.id, 'title', 'My Custom Title');

    mockedOpen.mockResolvedValue('/new');
    await store.locateMissingTracks();
    await flush();

    expect(track.path).toBe('/new/track.mp3');
    expect(track.title).toBe('My Custom Title');
  });
});
