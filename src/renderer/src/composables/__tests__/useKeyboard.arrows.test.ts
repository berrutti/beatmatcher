// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import { defineComponent, h } from 'vue';
import { mount } from '@vue/test-utils';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({
    isPlaying: false,
    isCueing: false,
    cuePointSec: 0,
    positionSec: 0,
    loopActive: false,
    loopRegionCleared: false,
    loopRegion: null,
    effectiveRate: 1
  })
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {})
}));

vi.mock('@tauri-apps/plugin-store', () => ({
  load: vi.fn().mockResolvedValue({})
}));

vi.mock('@renderer/utils/storage', () => ({
  storageGet: vi.fn((_key: string, fallback: unknown) => fallback),
  storageSet: vi.fn(),
  STORAGE_KEYS: {
    deckCount: 'deckCount',
    collectionHeight: 'collectionHeight',
    savedTracks: 'savedTracks',
    collection: 'collection',
    playlists: 'playlists',
    browserColumns: 'browserColumns',
    bigLibrary: 'bigLibrary',
    showWaveformStrip: 'showWaveformStrip'
  }
}));

import { useKeyboard } from '@renderer/composables/useKeyboard';
import { useBrowseStore } from '@renderer/stores/browse';
import { useSettingsStore } from '@renderer/stores/settings';
import { useAppModeStore } from '@renderer/stores/appMode';

const Host = defineComponent({
  setup() {
    useKeyboard();
    return () => h('div');
  }
});

function arrow(key: string, target?: EventTarget): KeyboardEvent {
  const event = new KeyboardEvent('keydown', {
    code: key,
    key,
    bubbles: true,
    cancelable: true
  });
  (target ?? window).dispatchEvent(event);
  return event;
}

function rangeInput(): HTMLInputElement {
  const input = document.createElement('input');
  input.type = 'range';
  document.body.appendChild(input);
  return input;
}

describe('arrow keys in performance mode', () => {
  let wrapper: ReturnType<typeof mount> | null = null;

  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    useAppModeStore().mode = 'performance';
    wrapper = mount(Host, { attachTo: document.body });
    const browse = useBrowseStore();
    browse.setRows('all', ['a.wav', 'b.wav', 'c.wav']);
    browse.moveCursor(1);
  });

  afterEach(() => {
    wrapper?.unmount();
    wrapper = null;
    document.body.innerHTML = '';
  });

  it('walks the browser when nothing else wants the key', () => {
    const browse = useBrowseStore();
    const before = browse.cursorIndex;

    const event = arrow('ArrowDown');

    expect(browse.cursorIndex).toBe(before + 1);
    expect(event.defaultPrevented).toBe(true);
  });

  it('leaves the key to a focused range input', () => {
    const browse = useBrowseStore();
    const before = browse.cursorIndex;

    const event = arrow('ArrowDown', rangeInput());

    expect(browse.cursorIndex).toBe(before);
    expect(event.defaultPrevented).toBe(false);
  });

  it('runs a deck command bound to an arrow instead of browsing', () => {
    const settings = useSettingsStore();
    settings.keybindings.A.PLAY = 'arrowleft';
    const browse = useBrowseStore();
    browse.openPlaylist('mix');
    const before = browse.activePlaylistId;

    arrow('ArrowLeft');

    expect(browse.activePlaylistId).toBe(before);
  });
});
