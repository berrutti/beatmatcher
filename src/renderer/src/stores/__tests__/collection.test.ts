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
    bigLibrary: 'bigLibrary'
  }
}));

import { useCollectionStore } from '../collection';
import { invoke } from '@tauri-apps/api/core';

const mockedInvoke = vi.mocked(invoke);

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
});
