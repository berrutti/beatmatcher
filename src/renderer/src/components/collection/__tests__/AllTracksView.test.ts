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

// Minimal stand-ins: the real stores pull in settings/session/WASM
// dependencies unrelated to what this test is about.
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
import AllTracksView from '../AllTracksView.vue';

async function flush() {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

async function mountWithReadyTrack() {
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

  const wrapper = mount(AllTracksView, {
    props: { tracks: store.tracks },
    global: {
      plugins: [i18n],
      directives: { tooltip: vTooltip },
      // Not under test here - stubbing keeps the mount focused on the row's
      // own pointerdown behavior.
      stubs: {
        Buttons: true,
        BpmModal: true,
        ConfirmModal: true,
        TrackBpmCell: true,
        TrackContextMenu: true,
        ColumnVisibilityMenu: true
      }
    }
  });

  return { wrapper, store };
}

describe('AllTracksView: drag must not let the browser auto-scroll the list', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('suppresses the pointerdown default action on a draggable (ready) track row', async () => {
    const { wrapper } = await mountWithReadyTrack();

    const row = wrapper.find('tr.collection__row');
    expect(row.exists()).toBe(true);

    const event = new PointerEvent('pointerdown', { button: 0, bubbles: true });
    const preventDefaultSpy = vi.spyOn(event, 'preventDefault');
    row.element.dispatchEvent(event);

    // Suppressing the default action stops WebKit's native text-selection
    // autoscroll, which fires with no wheel or scroll event to catch.
    expect(preventDefaultSpy).toHaveBeenCalled();
  });
});
