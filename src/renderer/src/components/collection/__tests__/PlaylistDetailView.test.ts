// @vitest-environment happy-dom
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

  it('leaves the pointerdown default action alone when the reorder grip is pressed', async () => {
    const { wrapper } = await mountWithPlaylistTrack();

    const grip = wrapper.find('.collection__playlist-grip');
    expect(grip.exists()).toBe(true);

    const event = new PointerEvent('pointerdown', { button: 0, bubbles: true });
    const preventDefaultSpy = vi.spyOn(event, 'preventDefault');
    grip.element.dispatchEvent(event);

    // Unlike drag-to-deck, this reorder-within-the-list drag needs the
    // browser's own autoscroll to reach the top of a long playlist.
    expect(preventDefaultSpy).not.toHaveBeenCalled();
  });

  it('suppresses it elsewhere on the row, where the press is a drag to a deck', async () => {
    const { wrapper } = await mountWithPlaylistTrack();

    const event = new PointerEvent('pointerdown', { button: 0, bubbles: true });
    const preventDefaultSpy = vi.spyOn(event, 'preventDefault');
    wrapper.find('.collection__meta-value--title').element.dispatchEvent(event);

    expect(preventDefaultSpy).toHaveBeenCalled();
  });
});

describe('PlaylistDetailView: reordering starts from the grip, not the whole row', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  function pointerDownOn(el: Element) {
    const event = new PointerEvent('pointerdown', { button: 0, bubbles: true });
    el.dispatchEvent(event);
  }

  it('arms the reorder when the press lands on the grip', async () => {
    const { wrapper } = await mountWithPlaylistTrack();

    const grip = wrapper.find('.collection__playlist-grip');
    expect(grip.exists()).toBe(true);
    pointerDownOn(grip.element);
    await wrapper.vm.$nextTick();

    expect(wrapper.find('.collection__playlist-track--dragging').exists()).toBe(true);
  });

  it('leaves a press on the track name alone, so it can be dragged to a deck', async () => {
    const { wrapper } = await mountWithPlaylistTrack();

    const title = wrapper.find('.collection__meta-value--title');
    expect(title.exists()).toBe(true);
    pointerDownOn(title.element);
    await wrapper.vm.$nextTick();

    expect(wrapper.find('.collection__playlist-track--dragging').exists()).toBe(false);
  });
});

describe('PlaylistDetailView: a playlist track can be dragged to a deck', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('starts a deck drag when the press on the row moves past the threshold', async () => {
    const { wrapper, store } = await mountWithPlaylistTrack();

    const title = wrapper.find('.collection__meta-value--title');
    expect(title.exists()).toBe(true);
    title.element.dispatchEvent(
      new PointerEvent('pointerdown', { button: 0, bubbles: true, clientX: 10, clientY: 10 })
    );
    window.dispatchEvent(
      new PointerEvent('pointermove', { bubbles: true, clientX: 200, clientY: 200 })
    );
    await wrapper.vm.$nextTick();

    expect(store.draggingPath).toBe('/music/track.mp3');
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }));
  });

  it('does not start a deck drag from the grip, which reorders instead', async () => {
    const { wrapper, store } = await mountWithPlaylistTrack();

    const grip = wrapper.find('.collection__playlist-grip');
    grip.element.dispatchEvent(
      new PointerEvent('pointerdown', { button: 0, bubbles: true, clientX: 10, clientY: 10 })
    );
    window.dispatchEvent(
      new PointerEvent('pointermove', { bubbles: true, clientX: 200, clientY: 200 })
    );
    await wrapper.vm.$nextTick();

    expect(store.draggingPath).toBe(null);
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }));
  });
});

describe('PlaylistDetailView: the two drags share no handler', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('binds the reorder to the grip and the deck drag to the row', async () => {
    const { wrapper } = await mountWithPlaylistTrack();

    // Separate listeners, so neither gesture can see or cancel the other's
    // press: a press on the grip never reaches the row's handler at all.
    const grip = wrapper.find('.collection__playlist-grip');
    grip.element.dispatchEvent(
      new PointerEvent('pointerdown', { button: 0, bubbles: true, clientX: 10, clientY: 10 })
    );
    window.dispatchEvent(
      new PointerEvent('pointermove', { bubbles: true, clientX: 200, clientY: 200 })
    );
    await wrapper.vm.$nextTick();

    const store = useCollectionStore();
    expect(store.draggingPath, 'a grip press must not arm the deck drag').toBe(null);
    expect(wrapper.find('.collection__playlist-track--dragging').exists()).toBe(true);
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }));
  });
});
