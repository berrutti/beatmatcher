import { describe, it, expect, vi, beforeEach } from 'vitest';
import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { i18n } from '@renderer/i18n';
import { vTooltip } from '@renderer/directives/tooltip';

// vi.hoisted avoids a top-level `import { invoke } from '@tauri-apps/api/core'`,
// which eslint restricts to store files only.
const mockedInvoke = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockedInvoke
}));

vi.mock('@renderer/utils/storage', () => ({
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

vi.mock('@renderer/stores/decks', () => ({
  useDecksStore: () => ({ bestAvailableDeck: vi.fn(() => 'A') }),
  DECKS_DISPOSITION: ['C', 'A', 'B', 'D']
}));
vi.mock('@renderer/stores/appMode', () => ({
  useAppModeStore: () => ({ mode: 'performance' })
}));
vi.mock('@renderer/stores/mixer', () => ({
  useMixerStore: () => ({ playedPaths: new Set() })
}));

import { useCollectionStore } from '@renderer/stores/collection';
import PlaylistDetailView from '../PlaylistDetailView.vue';

async function flush() {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

async function mountWithPlaylistTrack() {
  mockedInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'files_info') return [1000];
    if (cmd === 'analyze_track') return { bpm: 128, silenceEnd: 0.5 };
    if (cmd === 'read_track_tags') return { title: null, artist: null };
    throw new Error(`unexpected invoke: ${cmd}`);
  });

  const store = useCollectionStore();
  await store.addFilesFromPaths(['/music/track.mp3']);
  await flush();
  store.analyzeTrack(store.tracks[0].id);
  await flush();

  store.createPlaylist('Test playlist');
  const playlist = store.playlists[store.playlists.length - 1];
  const path = store.tracks[0].path;
  if (!path) throw new Error('expected a path on the seeded track');
  store.addToPlaylist(playlist.id, path);

  const wrapper = mount(PlaylistDetailView, {
    props: { playlistId: playlist.id },
    global: {
      plugins: [i18n],
      directives: { tooltip: vTooltip },
      stubs: {
        Buttons: true,
        BpmModal: true,
        TrackBpmCell: true,
        TrackStatusTag: true,
        TrackContextMenu: true,
        ColumnVisibilityMenu: true
      }
    }
  });

  return { wrapper, store };
}

describe('PlaylistDetailView: drag must still let the browser auto-scroll the list', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('leaves the pointerdown default action alone on a playlist track row', async () => {
    const { wrapper } = await mountWithPlaylistTrack();

    const row = wrapper.find('tr.collection__playlist-track');
    expect(row.exists()).toBe(true);

    const event = new PointerEvent('pointerdown', { button: 0, bubbles: true });
    const preventDefaultSpy = vi.spyOn(event, 'preventDefault');
    row.element.dispatchEvent(event);

    // Unlike drag-to-deck, this reorder-within-the-list drag needs the
    // browser's own autoscroll to reach the top of a long playlist.
    expect(preventDefaultSpy).not.toHaveBeenCalled();
  });
});
