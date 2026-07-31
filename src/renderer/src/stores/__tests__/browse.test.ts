import { describe, it, expect, vi, beforeEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn()
}));

vi.mock('@renderer/utils/deckDrop', () => ({
  loadToDeck: vi.fn()
}));

import { useBrowseStore, playlistListId } from '../browse';
import { loadToDeck } from '@renderer/utils/deckDrop';

const mockedLoadToDeck = vi.mocked(loadToDeck);

describe('the browse cursor', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('stays on the same track when the list re-sorts', () => {
    const browse = useBrowseStore();
    browse.setRows('all', ['/a.mp3', '/b.mp3', '/c.mp3']);
    browse.moveCursor(1);
    browse.moveCursor(1);

    expect(browse.cursorKey).toBe('/b.mp3');

    browse.setRows('all', ['/c.mp3', '/b.mp3', '/a.mp3']);

    expect(browse.cursorKey).toBe('/b.mp3');
    expect(browse.cursorIndex).toBe(1);
  });

  it('loses the highlight while a filter excludes the anchored track, and takes it back after', () => {
    const browse = useBrowseStore();
    browse.setRows('all', ['/a.mp3', '/b.mp3', '/c.mp3']);
    browse.moveCursor(1);
    browse.moveCursor(2);
    expect(browse.cursorKey).toBe('/c.mp3');

    browse.setRows('all', ['/a.mp3']);

    expect(browse.cursorIndex).toBe(-1);
    expect(browse.cursorKey).toBeNull();

    browse.setRows('all', ['/a.mp3', '/b.mp3', '/c.mp3']);

    expect(browse.cursorKey).toBe('/c.mp3');
  });

  it('clamps at both ends and starts from the top with nothing anchored', () => {
    const browse = useBrowseStore();
    browse.setRows('all', ['/a.mp3', '/b.mp3']);

    browse.moveCursor(1);
    expect(browse.cursorKey).toBe('/a.mp3');

    browse.moveCursor(-1);
    expect(browse.cursorKey).toBe('/a.mp3');

    browse.moveCursor(5);
    expect(browse.cursorKey).toBe('/b.mp3');
  });

  it('starts from the bottom when the first move is upwards', () => {
    const browse = useBrowseStore();
    browse.setRows('all', ['/a.mp3', '/b.mp3']);

    browse.moveCursor(-1);

    expect(browse.cursorKey).toBe('/b.mp3');
  });

  it('keeps one anchor per list across leaving and re-entering a playlist', () => {
    const browse = useBrowseStore();
    browse.setTab('playlists');
    browse.setRows('playlists', ['one', 'two']);
    browse.moveCursor(1);
    browse.enter();

    expect(browse.activePlaylistId).toBe('one');

    browse.setRows(playlistListId('one'), ['/a.mp3', '/b.mp3']);
    browse.moveCursor(1);
    browse.moveCursor(1);
    expect(browse.cursorKey).toBe('/b.mp3');

    browse.back();
    browse.setRows('playlists', ['one', 'two']);
    expect(browse.cursorKey).toBe('one');

    browse.enter();
    browse.setRows(playlistListId('one'), ['/a.mp3', '/b.mp3']);
    expect(browse.cursorKey).toBe('/b.mp3');
  });

  it('ignores rows pushed by a list that is not on screen', () => {
    const browse = useBrowseStore();
    browse.setRows('all', ['/a.mp3', '/b.mp3']);
    browse.setRows('playlists', ['one', 'two']);

    expect(browse.rows).toEqual(['/a.mp3', '/b.mp3']);
  });

  it('walks back out of a playlist to the overview and then to all tracks', () => {
    const browse = useBrowseStore();
    browse.openPlaylist('one');

    browse.back();
    expect(browse.tab).toBe('playlists');
    expect(browse.activePlaylistId).toBeNull();

    browse.back();
    expect(browse.tab).toBe('all');

    browse.back();
    expect(browse.tab).toBe('all');
  });

  it('toggles between the two views and keeps the open playlist', () => {
    const browse = useBrowseStore();
    browse.openPlaylist('one');

    browse.toggleView();
    expect(browse.tab).toBe('all');

    browse.toggleView();
    expect(browse.tab).toBe('playlists');
    expect(browse.activePlaylistId).toBe('one');
  });

  it('loads the cursor track into the deck a load button names', () => {
    const browse = useBrowseStore();
    browse.setRows('all', ['/a.mp3', '/b.mp3']);
    browse.moveCursor(1);
    browse.moveCursor(1);

    browse.loadCursorInto('B');

    expect(mockedLoadToDeck).toHaveBeenCalledWith('/b.mp3', 'B');
  });

  it('loads nothing from the playlists overview or with no cursor', () => {
    const browse = useBrowseStore();
    browse.setRows('all', ['/a.mp3']);
    browse.loadCursorInto('A');

    browse.setTab('playlists');
    browse.setRows('playlists', ['one']);
    browse.moveCursor(1);
    browse.loadCursorInto('A');

    expect(mockedLoadToDeck).not.toHaveBeenCalled();
  });
});
