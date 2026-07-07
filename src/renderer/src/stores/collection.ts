import { defineStore } from 'pinia';
import { reactive, ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { LoadableTrack } from '@renderer/stores/decks';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import { indexByBasename } from '@renderer/utils/path';

type CollectionEntryStatus = 'idle' | 'analyzing' | 'ready' | 'error' | 'missing';

export type SavedTrack = {
  path: string;
  name: string;
  bpm: number;
  silenceEnd: number;
  beatOffset: number;
};

export type Playlist = {
  id: string;
  name: string;
  paths: string[];
  addedAt: Record<string, number>;
};

// Every editable, non-BPM metadata point a track can carry. `title` and
// `artist` are shown by default; the rest are opt-in columns the user picks
// from the header context menu.
export const METADATA_FIELDS = [
  'title',
  'artist',
  'album',
  'albumArtist',
  'genre',
  'composer',
  'remixer',
  'label',
  'comment',
  'trackNumber',
  'year',
  'rating'
] as const;
export type MetadataField = (typeof METADATA_FIELDS)[number];
export type TrackMetadata = Record<MetadataField, string | null>;

const DEFAULT_VISIBLE_COLUMNS: MetadataField[] = ['title', 'artist'];

// Title gets a narrower default than the other text fields since it's
// usually the shortest of the bunch; the rest default to a size that fits
// typical values without being read from disk.
const DEFAULT_COLUMN_WIDTH: Record<MetadataField, number> = {
  title: 140,
  artist: 130,
  album: 130,
  albumArtist: 130,
  genre: 100,
  composer: 130,
  remixer: 130,
  label: 110,
  comment: 160,
  trackNumber: 70,
  year: 70,
  rating: 70
};

type ColumnsState = {
  order: MetadataField[];
  visible: MetadataField[];
  widths: Partial<Record<MetadataField, number>>;
};

export function isMetadataField(value: unknown): value is MetadataField {
  return typeof value === 'string' && METADATA_FIELDS.some((field) => field === value);
}

function loadColumnsState(): ColumnsState {
  const fallback: ColumnsState = {
    order: [...METADATA_FIELDS],
    visible: [...DEFAULT_VISIBLE_COLUMNS],
    widths: {}
  };
  const stored = storageGet<Partial<ColumnsState> | null>(STORAGE_KEYS.browserColumns, null);
  if (!stored || !Array.isArray(stored.order) || !Array.isArray(stored.visible)) return fallback;
  const validOrder = stored.order.filter(isMetadataField);
  // A field added to METADATA_FIELDS after this was saved won't be in the
  // stored order yet; append it so it still shows up in the column picker.
  const order = [...validOrder, ...METADATA_FIELDS.filter((f) => !validOrder.includes(f))];
  const visible = stored.visible.filter(isMetadataField);
  return { order, visible, widths: stored.widths ?? {} };
}

export type CollectionEntry = {
  id: string;
  name: string;
  size: number;
  path: string | null;
  status: CollectionEntryStatus;
  silenceEnd: number;
  addedAt: number | null;
  lastAnalysisFailed: boolean;
} & TrackMetadata;

function emptyMetadata(): TrackMetadata {
  return {
    title: null,
    artist: null,
    album: null,
    albumArtist: null,
    genre: null,
    composer: null,
    remixer: null,
    label: null,
    comment: null,
    trackNumber: null,
    year: null,
    rating: null
  };
}

type PersistedEntry = {
  name: string;
  size: number;
  path: string | null;
  addedAt: number | null;
};

function persistCollection(entries: CollectionEntry[]) {
  storageSet(
    STORAGE_KEYS.collection,
    entries.map((t) => ({ name: t.name, size: t.size, path: t.path, addedAt: t.addedAt }))
  );
}

export const useCollectionStore = defineStore('collection', () => {
  const stored = storageGet<Record<string, SavedTrack>>(STORAGE_KEYS.savedTracks, {});
  const savedTracks = reactive<Record<string, SavedTrack>>(
    typeof stored === 'object' && stored !== null ? stored : {}
  );

  function persistSaved() {
    storageSet(STORAGE_KEYS.savedTracks, savedTracks);
  }

  function getSaved(path: string): SavedTrack | null {
    return savedTracks[path] ?? null;
  }

  function saveSaved(track: SavedTrack) {
    savedTracks[track.path] = track;
    persistSaved();
  }

  function updateSaved(path: string, patch: Partial<Omit<SavedTrack, 'path'>>) {
    const existing = savedTracks[path];
    if (!existing) return;
    savedTracks[path] = { ...existing, ...patch };
    persistSaved();
  }

  function removeSaved(path: string) {
    delete savedTracks[path];
    persistSaved();
  }

  const bigLibrary = ref(storageGet<boolean>(STORAGE_KEYS.bigLibrary, false));
  const tracks = reactive<CollectionEntry[]>([]);
  const draggingPath = ref<string | null>(null);

  function toggleBigLibrary() {
    bigLibrary.value = !bigLibrary.value;
    storageSet(STORAGE_KEYS.bigLibrary, bigLibrary.value);
  }

  const columnsState = reactive<ColumnsState>(loadColumnsState());

  function persistColumnsState() {
    storageSet(STORAGE_KEYS.browserColumns, columnsState);
  }

  const orderedVisibleColumns = computed(() =>
    columnsState.order.filter((f) => columnsState.visible.includes(f))
  );

  function isColumnVisible(field: MetadataField): boolean {
    return columnsState.visible.includes(field);
  }

  function toggleColumn(field: MetadataField) {
    const idx = columnsState.visible.indexOf(field);
    if (idx !== -1) {
      // At least one column must stay visible, or the table would have
      // nothing left to show and no way to bring a column back.
      if (columnsState.visible.length === 1) return;
      columnsState.visible.splice(idx, 1);
    } else {
      columnsState.visible.push(field);
    }
    persistColumnsState();
  }

  // `beforeField` is the field `field` should land in front of, or null to
  // move it to the end. Working in terms of fields rather than indices keeps
  // this correct regardless of how many hidden columns sit between them.
  function reorderColumn(field: MetadataField, beforeField: MetadataField | null) {
    const order = columnsState.order;
    const fromIndex = order.indexOf(field);
    if (fromIndex === -1) return;
    order.splice(fromIndex, 1);
    const toIndex = beforeField !== null ? order.indexOf(beforeField) : order.length;
    order.splice(toIndex, 0, field);
    persistColumnsState();
  }

  function getColumnWidth(field: MetadataField): number {
    return columnsState.widths[field] ?? DEFAULT_COLUMN_WIDTH[field];
  }

  function setColumnWidth(field: MetadataField, widthPx: number) {
    columnsState.widths[field] = Math.max(40, Math.round(widthPx));
    persistColumnsState();
  }

  const hasPending = computed(() => tracks.some((t) => t.status === 'idle'));

  function getBpm(entry: CollectionEntry): number | null {
    return entry.path ? (getSaved(entry.path)?.bpm ?? null) : null;
  }

  const persistedEntries = storageGet<PersistedEntry[]>(STORAGE_KEYS.collection, []);
  persistedEntries.forEach((p) => {
    tracks.push({
      id: `${p.name}-${Math.random().toString(36).slice(2)}`,
      name: p.name,
      size: p.size,
      path: p.path,
      status: 'missing',
      silenceEnd: 0,
      ...emptyMetadata(),
      addedAt: p.addedAt ?? null,
      lastAnalysisFailed: false
    });
  });

  async function checkInitialFileSizes() {
    const pathsWithIdx = tracks
      .map((t, i) => (t.path ? { path: t.path, i } : null))
      .filter((x): x is { path: string; i: number } => x !== null);
    if (pathsWithIdx.length === 0) return;
    try {
      const sizes = await invoke<(number | null)[]>('files_info', {
        paths: pathsWithIdx.map((x) => x.path)
      });
      pathsWithIdx.forEach(({ path, i }, k) => {
        const size = sizes[k];
        if (size === null || size === undefined) return;
        const entry = tracks[i];
        const hasSaved = getSaved(path) !== null;
        entry.size = size;
        entry.status = hasSaved ? 'ready' : 'idle';
        queueTagRead(entry.id);
      });
    } catch (err) {
      console.error('[collection] failed to check initial file sizes:', err);
    }
  }

  if (tracks.length > 0) checkInitialFileSizes();

  watch(
    () => tracks.map((t) => ({ name: t.name, size: t.size, path: t.path })),
    () => persistCollection(tracks),
    { deep: true }
  );

  async function addFilesFromPaths(paths: string[]) {
    const newPaths = paths.filter((p) => !tracks.some((t) => t.path === p));
    if (newPaths.length === 0) return;
    const sizes = await invoke<(number | null)[]>('files_info', { paths: newPaths });
    newPaths.forEach((path, i) => {
      const size = sizes[i];
      if (size === null || size === undefined) return;
      const name = path.split('/').pop() ?? path;
      const hasSaved = getSaved(path) !== null;
      const entry: CollectionEntry = {
        id: `${name}-${Math.random().toString(36).slice(2)}`,
        name,
        size,
        path,
        status: hasSaved ? 'ready' : 'idle',
        silenceEnd: 0,
        ...emptyMetadata(),
        addedAt: Date.now(),
        lastAnalysisFailed: false
      };
      tracks.push(entry);
      queueTagRead(entry.id);
    });
  }

  function addFiles(files: File[]) {
    for (const file of files) {
      const path = (file as File & { path?: string }).path ?? null;
      const existing = tracks.find((t) => t.name === file.name && t.size === file.size);
      if (existing) {
        if (existing.status === 'missing') {
          existing.path = path ?? existing.path;
          const hasSaved = existing.path !== null && getSaved(existing.path) !== null;
          existing.status = hasSaved ? 'ready' : 'idle';
          queueTagRead(existing.id);
        }
        continue;
      }
      const hasSaved = path !== null && getSaved(path) !== null;
      const entry: CollectionEntry = {
        id: `${file.name}-${Math.random().toString(36).slice(2)}`,
        name: file.name,
        size: file.size,
        path,
        status: hasSaved ? 'ready' : 'idle',
        silenceEnd: 0,
        ...emptyMetadata(),
        addedAt: Date.now(),
        lastAnalysisFailed: false
      };
      tracks.push(entry);
      queueTagRead(entry.id);
    }
  }

  function relinkEntry(entry: CollectionEntry, newPath: string, size: number) {
    if (tracks.some((t) => t.id !== entry.id && t.path === newPath)) {
      const idx = tracks.findIndex((t) => t.id === entry.id);
      if (idx !== -1) tracks.splice(idx, 1);
      return;
    }

    const oldPath = entry.path;
    if (oldPath && oldPath !== newPath) {
      const saved = savedTracks[oldPath];
      if (saved) {
        savedTracks[newPath] = { ...saved, path: newPath };
        delete savedTracks[oldPath];
        persistSaved();
      }
      for (const p of playlists) {
        const idx = p.paths.indexOf(oldPath);
        if (idx !== -1) p.paths[idx] = newPath;
      }
    }

    entry.path = newPath;
    entry.size = size;
    entry.status = getSaved(newPath) !== null ? 'ready' : 'idle';
    queueTagRead(entry.id);
  }

  // Opens a folder picker and relinks every missing entry whose filename is
  // found under it (recursively), so one pick fixes a whole moved library.
  // Saved BPM/grid data and playlist references follow the path, so nothing
  // has to be re-analyzed.
  async function locateMissingTracks(): Promise<void> {
    if (!tracks.some((t) => t.status === 'missing')) return;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const folder = await open({ directory: true, multiple: false });
    if (typeof folder !== 'string') return;

    const found = await invoke<string[]>('scan_folder', { path: folder }).catch(
      () => [] as string[]
    );
    const byName = indexByBasename(found);

    const targets = tracks
      .filter((t) => t.status === 'missing')
      .map((t) => ({ entry: t, newPath: byName.get(t.name) }))
      .filter((t): t is { entry: CollectionEntry; newPath: string } => t.newPath !== undefined);
    if (targets.length === 0) return;

    const sizes = await invoke<(number | null)[]>('files_info', {
      paths: targets.map((t) => t.newPath)
    });
    targets.forEach(({ entry: target, newPath }, i) => {
      const size = sizes[i];
      if (size !== null && size !== undefined) relinkEntry(target, newPath, size);
    });
  }

  function removeTrack(id: string) {
    const idx = tracks.findIndex((t) => t.id === id);
    if (idx !== -1) {
      const path = tracks[idx].path;
      if (path) removeSaved(path);
      tracks.splice(idx, 1);
    }
  }

  function reanalyzeTrack(id: string) {
    const entry = tracks.find((t) => t.id === id);
    if (!entry || !entry.path) return;
    // The previous BPM is kept (not wiped) until a new analysis actually
    // succeeds, so a failed reanalysis doesn't throw away good data.
    entry.status = 'idle';
    analyzeTrack(id);
  }

  function clearAll() {
    tracks.splice(0, tracks.length);
  }

  function createConcurrentQueue(concurrency: number, worker: (id: string) => Promise<void>) {
    let active = 0;
    const pending: string[] = [];

    function drain() {
      while (active < concurrency && pending.length > 0) {
        const id = pending.shift();
        if (id === undefined) continue;
        active++;
        run(id);
      }
    }

    async function run(id: string) {
      await worker(id);
      active--;
      drain();
    }

    return {
      enqueue(id: string) {
        pending.push(id);
        drain();
      },
      get pending() {
        return pending;
      }
    };
  }

  async function doAnalyze(id: string) {
    const entry = tracks.find((t) => t.id === id);
    if (!entry || !entry.path || entry.status !== 'idle') return;
    const hadPreviousBpm = getBpm(entry) !== null;
    entry.status = 'analyzing';
    try {
      const result = await invoke<{ bpm: number | null; silenceEnd: number }>('analyze_track', {
        path: entry.path
      });
      entry.silenceEnd = result.silenceEnd;
      if (result.bpm !== null && result.bpm > 0) {
        saveSaved({
          path: entry.path,
          name: entry.name,
          bpm: result.bpm,
          silenceEnd: result.silenceEnd,
          beatOffset: result.silenceEnd
        });
        entry.status = 'ready';
        entry.lastAnalysisFailed = false;
      } else if (hadPreviousBpm) {
        entry.status = 'ready';
        entry.lastAnalysisFailed = true;
      } else {
        entry.status = 'error';
      }
    } catch {
      if (hadPreviousBpm) {
        entry.status = 'ready';
        entry.lastAnalysisFailed = true;
      } else {
        entry.status = 'error';
      }
    }
  }

  async function readTagsForEntry(entry: CollectionEntry) {
    if (!entry.path) return;
    try {
      const tags = await invoke<TrackMetadata>('read_track_tags', { path: entry.path });
      const overridesForPath = metadataOverrides[entry.path] ?? {};
      for (const field of METADATA_FIELDS) {
        entry[field] = overridesForPath[field] ?? tags[field] ?? null;
      }
    } catch {
      // ignore tag read failures
    }
  }

  // Metadata edits are app-only: they never touch the file's own tags (Rust
  // can only read tags today, not write them; see TODO below), so they're
  // kept as overrides layered on top of whatever readTagsForEntry reads next.
  //
  // TODO: once Rust gains tag-writing support, editing metadata here should
  // write back into the actual file tags (like Rekordbox/Mixxx/Serato do),
  // with this override map becoming the write queue instead of a permanent
  // shadow copy.
  const metadataOverrides = reactive<Record<string, Partial<TrackMetadata>>>(
    storageGet(STORAGE_KEYS.metadataOverrides, {})
  );

  function persistMetadataOverrides() {
    storageSet(STORAGE_KEYS.metadataOverrides, metadataOverrides);
  }

  function setMetadataField(id: string, field: MetadataField, value: string | null) {
    const entry = tracks.find((t) => t.id === id);
    if (!entry || !entry.path) return;
    entry[field] = value;
    const overridesForPath = metadataOverrides[entry.path] ?? {};
    overridesForPath[field] = value;
    metadataOverrides[entry.path] = overridesForPath;
    persistMetadataOverrides();
  }

  const analysisQueue = createConcurrentQueue(3, doAnalyze);
  const tagQueue = createConcurrentQueue(8, async (id) => {
    const entry = tracks.find((t) => t.id === id);
    if (entry) await readTagsForEntry(entry);
  });

  function analyzeTrack(id: string) {
    const entry = tracks.find((t) => t.id === id);
    if (!entry || !entry.path || entry.status === 'analyzing' || entry.status === 'missing') return;
    analysisQueue.enqueue(id);
  }

  function analyzeAll() {
    for (const track of tracks.filter((t) => t.status === 'idle')) {
      analysisQueue.enqueue(track.id);
    }
  }

  function queueTagRead(id: string) {
    tagQueue.enqueue(id);
  }

  function setBpm(id: string, bpm: number) {
    const entry = tracks.find((t) => t.id === id);
    if (!entry || !entry.path || bpm <= 0) return;
    saveSaved({
      path: entry.path,
      name: entry.name,
      bpm,
      silenceEnd: entry.silenceEnd,
      beatOffset: entry.silenceEnd
    });
    entry.status = 'ready';
    entry.lastAnalysisFailed = false;
  }

  function updateTrack(path: string, patch: { beatOffset?: number; bpm?: number }) {
    updateSaved(path, patch);
  }

  function displayLabel(entry: Pick<CollectionEntry, 'title' | 'artist'>): string | null {
    const parts = [entry.artist, entry.title].filter(Boolean);
    return parts.length > 0 ? parts.join(' - ') : null;
  }

  function getName(path: string): string {
    const entry = tracks.find((t) => t.path === path);
    if (entry) return displayLabel(entry) ?? entry.name;
    return (
      path
        .split('/')
        .pop()
        ?.replace(/\.[^.]+$/, '') ?? path
    );
  }

  function getLoadable(path: string): Omit<LoadableTrack, 'onBeatOffsetChange'> | null {
    const saved = getSaved(path);
    if (!saved) return null;
    const entry = tracks.find((t) => t.path === path);
    return {
      path,
      name: (entry && displayLabel(entry)) ?? saved.name,
      bpm: saved.bpm,
      silenceEnd: saved.silenceEnd,
      beatOffset: saved.beatOffset
    };
  }

  function getLoadableTrack(path: string): LoadableTrack | null {
    const data = getLoadable(path);
    return data
      ? {
          ...data,
          onBeatOffsetChange: (sec) => updateTrack(path, { beatOffset: sec })
        }
      : null;
  }

  async function scanFolders(folders: string[]): Promise<string[]> {
    const pathLists = await Promise.all(
      folders.map((folder) => invoke<string[]>('scan_folder', { path: folder }))
    );
    return pathLists.flat();
  }

  function startDrag(path: string) {
    draggingPath.value = path;
  }

  function endDrag() {
    draggingPath.value = null;
  }

  const loadedPlaylists = storageGet<Playlist[]>(STORAGE_KEYS.playlists, []);
  // Playlists saved before per-playlist "date added" existed have no
  // addedAt map at all; backfill an empty one so lookups never throw. Paths
  // already in the playlist at that point have no recorded moment, and
  // correctly show as unknown rather than inventing one.
  loadedPlaylists.forEach((p) => {
    if (!p.addedAt) p.addedAt = {};
  });
  const playlists = reactive<Playlist[]>(loadedPlaylists);

  watch(
    playlists,
    () => {
      storageSet(STORAGE_KEYS.playlists, playlists);
    },
    { deep: true }
  );

  function createPlaylist(name: string) {
    playlists.push({ id: Math.random().toString(36).slice(2), name, paths: [], addedAt: {} });
  }

  function deletePlaylist(id: string) {
    const idx = playlists.findIndex((p) => p.id === id);
    if (idx !== -1) playlists.splice(idx, 1);
  }

  function renamePlaylist(id: string, name: string) {
    const p = playlists.find((p) => p.id === id);
    if (p) p.name = name;
  }

  function addToPlaylist(playlistId: string, path: string) {
    const p = playlists.find((p) => p.id === playlistId);
    if (!p || p.paths.includes(path)) return;
    p.paths.push(path);
    p.addedAt[path] = Date.now();
  }

  function removeFromPlaylist(playlistId: string, path: string) {
    const p = playlists.find((p) => p.id === playlistId);
    if (!p) return;
    const idx = p.paths.indexOf(path);
    if (idx !== -1) p.paths.splice(idx, 1);
    delete p.addedAt[path];
  }

  function moveInPlaylist(playlistId: string | null, fromIdx: number, toIdx: number) {
    const p = playlists.find((p) => p.id === playlistId);
    if (!p || fromIdx === toIdx) return;
    const [item] = p.paths.splice(fromIdx, 1);
    p.paths.splice(toIdx, 0, item);
  }

  return {
    draggingPath,
    hasPending,
    bigLibrary,
    playlists,
    tracks,
    addFiles,
    addFilesFromPaths,
    addToPlaylist,
    analyzeAll,
    analyzeTrack,
    clearAll,
    createPlaylist,
    deletePlaylist,
    endDrag,
    getBpm,
    getName,
    getSaved,
    getLoadableTrack,
    locateMissingTracks,
    moveInPlaylist,
    reanalyzeTrack,
    removeFromPlaylist,
    removeTrack,
    renamePlaylist,
    scanFolders,
    setBpm,
    setMetadataField,
    startDrag,
    toggleBigLibrary,
    updateTrack,
    columnOrder: computed(() => columnsState.order),
    orderedVisibleColumns,
    isColumnVisible,
    toggleColumn,
    reorderColumn,
    getColumnWidth,
    setColumnWidth
  };
});
