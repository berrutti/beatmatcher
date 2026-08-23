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
    browserColumns: 'browserColumns'
  }
}));

import { storageGet } from '@renderer/utils/storage';
import { useCollectionStore, METADATA_FIELDS, COLUMN_FIELDS, isMetadataField } from '../collection';

const mockedStorageGet = vi.mocked(storageGet);

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
    for (const field of COLUMN_FIELDS) {
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
    expect(store.orderedVisibleColumns).toEqual(['title', 'bpm', 'added']);
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
    // title and artist are visible by default. Genre/album are hidden and
    // sit between them in the default METADATA_FIELDS order.
    store.reorderColumn('artist', 'title');
    const order = store.columnOrder;
    expect(order.indexOf('artist')).toBeLessThan(order.indexOf('title'));
    expect(store.orderedVisibleColumns).toEqual(['artist', 'title', 'bpm', 'added']);
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

describe('collection store: column shares', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it('falls back to a sensible default share before any resize', () => {
    const store = useCollectionStore();
    expect(store.getColumnShare('title')).toBeGreaterThan(0);
  });

  it('persists a manually set share', () => {
    const store = useCollectionStore();
    store.setColumnShare('title', 200);
    expect(store.getColumnShare('title')).toBe(200);
  });

  it('clamps shares to a sane minimum instead of allowing zero/negative shares', () => {
    const store = useCollectionStore();
    store.setColumnShare('title', -50);
    expect(store.getColumnShare('title')).toBeGreaterThan(0);
  });
});

describe('collection store: bpm/added as adjustable columns', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it('shows bpm and added by default alongside title and artist', () => {
    const store = useCollectionStore();
    expect(store.isColumnVisible('bpm')).toBe(true);
    expect(store.isColumnVisible('added')).toBe(true);
  });

  it('toggles bpm hidden and back to visible', () => {
    const store = useCollectionStore();
    store.toggleColumn('bpm');
    expect(store.isColumnVisible('bpm')).toBe(false);
    store.toggleColumn('bpm');
    expect(store.isColumnVisible('bpm')).toBe(true);
  });

  it('reorders bpm in front of a metadata column', () => {
    const store = useCollectionStore();
    store.reorderColumn('bpm', 'title');
    expect(store.columnOrder.indexOf('bpm')).toBeLessThan(store.columnOrder.indexOf('title'));
    expect(store.orderedVisibleColumns.indexOf('bpm')).toBeLessThan(
      store.orderedVisibleColumns.indexOf('title')
    );
  });
});

describe('collection store: bpm/added migration for pre-existing installs', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('keeps bpm and added visible for a store saved before they were adjustable columns', () => {
    mockedStorageGet.mockImplementation((key: string, fallback: unknown) => {
      if (key === 'browserColumns') {
        return { order: ['title', 'artist'], visible: ['title'], widths: {} };
      }
      return fallback;
    });

    const store = useCollectionStore();

    expect(store.isColumnVisible('bpm')).toBe(true);
    expect(store.isColumnVisible('added')).toBe(true);
    expect(store.columnOrder).toContain('bpm');
    expect(store.columnOrder).toContain('added');
  });

  it('respects an explicit bpm/added visibility choice made after the migration already ran', () => {
    mockedStorageGet.mockImplementation((key: string, fallback: unknown) => {
      if (key === 'browserColumns') {
        return {
          order: ['title', 'artist', 'bpm', 'added'],
          visible: ['title', 'added'],
          widths: {}
        };
      }
      return fallback;
    });

    const store = useCollectionStore();

    expect(store.isColumnVisible('bpm')).toBe(false);
    expect(store.isColumnVisible('added')).toBe(true);
  });
});

describe('collection store: widths-to-shares migration', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('reuses a pre-existing pixel width as the initial share for that column', () => {
    mockedStorageGet.mockImplementation((key: string, fallback: unknown) => {
      if (key === 'browserColumns') {
        return {
          order: ['title', 'artist', 'bpm', 'added'],
          visible: ['title', 'artist', 'bpm', 'added'],
          widths: { title: 200, artist: 100 }
        };
      }
      return fallback;
    });

    const store = useCollectionStore();

    expect(store.getColumnShare('title')).toBe(200);
    expect(store.getColumnShare('artist')).toBe(100);
  });

  it('prefers an already-migrated shares key over a stale widths key', () => {
    mockedStorageGet.mockImplementation((key: string, fallback: unknown) => {
      if (key === 'browserColumns') {
        return {
          order: ['title', 'artist', 'bpm', 'added'],
          visible: ['title', 'artist', 'bpm', 'added'],
          shares: { title: 300 },
          widths: { title: 200 }
        };
      }
      return fallback;
    });

    const store = useCollectionStore();

    expect(store.getColumnShare('title')).toBe(300);
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
