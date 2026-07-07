import { describe, it, expect, vi, beforeEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}));

vi.mock('@renderer/utils/storage', () => ({
  // Mirrors real storageGet's behavior for a fresh install: nothing is in
  // localStorage yet, so every call falls back to its own default.
  storageGet: vi.fn().mockImplementation((_key: string, fallback: unknown) => fallback),
  storageSet: vi.fn(),
  STORAGE_KEYS: {
    collection: 'collection',
    playlists: 'playlists',
    savedTracks: 'savedTracks',
    bigLibrary: 'bigLibrary',
    browserColumns: 'browserColumns',
    playlistListColumns: 'playlistListColumns'
  }
}));

import { useCollectionStore, METADATA_FIELDS, isMetadataField } from '../collection';

describe('collection store: column visibility', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it('shows title and artist by default', () => {
    const store = useCollectionStore();
    expect(store.isColumnVisible('title')).toBe(true);
    expect(store.isColumnVisible('artist')).toBe(true);
    expect(store.isColumnVisible('genre')).toBe(false);
  });

  it('toggles a column visible and back to hidden', () => {
    const store = useCollectionStore();
    store.toggleColumn('genre');
    expect(store.isColumnVisible('genre')).toBe(true);
    store.toggleColumn('genre');
    expect(store.isColumnVisible('genre')).toBe(false);
  });

  it('refuses to hide the last visible column', () => {
    const store = useCollectionStore();
    for (const field of METADATA_FIELDS) {
      if (store.isColumnVisible(field) && field !== 'title') store.toggleColumn(field);
    }
    expect(store.orderedVisibleColumns).toEqual(['title']);

    store.toggleColumn('title');

    expect(store.isColumnVisible('title')).toBe(true);
    expect(store.orderedVisibleColumns).toEqual(['title']);
  });

  it('never leaves zero visible columns no matter which one is hidden last', () => {
    const store = useCollectionStore();
    store.toggleColumn('artist');
    expect(store.orderedVisibleColumns).toEqual(['title']);
    store.toggleColumn('title');
    expect(store.orderedVisibleColumns.length).toBeGreaterThanOrEqual(1);
  });
});

describe('collection store: column reordering', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it('moves a field in front of another field', () => {
    const store = useCollectionStore();
    store.reorderColumn('genre', 'title');
    expect(store.columnOrder.indexOf('genre')).toBeLessThan(store.columnOrder.indexOf('title'));
  });

  it('moves a field to the end when beforeField is null', () => {
    const store = useCollectionStore();
    store.reorderColumn('title', null);
    expect(store.columnOrder[store.columnOrder.length - 1]).toBe('title');
  });

  it('reorders correctly regardless of hidden columns interspersed between the two fields', () => {
    const store = useCollectionStore();
    // title and artist are visible by default; genre/album are hidden and
    // sit between them in the default METADATA_FIELDS order.
    store.reorderColumn('artist', 'title');
    const order = store.columnOrder;
    expect(order.indexOf('artist')).toBeLessThan(order.indexOf('title'));
    // the visible order (what the table actually renders) reflects the move
    expect(store.orderedVisibleColumns).toEqual(['artist', 'title']);
  });

  it('is a no-op for a field that is not in the order (defensive check)', () => {
    const store = useCollectionStore();
    const before = [...store.columnOrder];
    // @ts-expect-error deliberately passing a value outside the union to
    // exercise the fromIndex === -1 guard
    store.reorderColumn('not-a-field', 'title');
    expect(store.columnOrder).toEqual(before);
  });
});

describe('collection store: column widths', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it('falls back to a sensible default width before any resize', () => {
    const store = useCollectionStore();
    expect(store.getColumnWidth('title')).toBeGreaterThan(0);
  });

  it('persists a manually set width', () => {
    const store = useCollectionStore();
    store.setColumnWidth('title', 200);
    expect(store.getColumnWidth('title')).toBe(200);
  });

  it('clamps widths to a sane minimum instead of allowing zero/negative widths', () => {
    const store = useCollectionStore();
    store.setColumnWidth('title', -50);
    expect(store.getColumnWidth('title')).toBeGreaterThanOrEqual(40);
  });
});

describe('isMetadataField', () => {
  it('accepts every known metadata field', () => {
    for (const field of METADATA_FIELDS) expect(isMetadataField(field)).toBe(true);
  });

  it('rejects unknown strings and non-strings', () => {
    expect(isMetadataField('not-a-field')).toBe(false);
    expect(isMetadataField(null)).toBe(false);
    expect(isMetadataField(42)).toBe(false);
  });
});
