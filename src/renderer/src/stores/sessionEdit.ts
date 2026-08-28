import { ref, computed, watch } from 'vue';
import { defineStore } from 'pinia';
import { STORAGE_KEYS, storageGet, storageSet } from '@renderer/utils/storage';
import { call } from '@renderer/tauriCommands';
import { useSessionStore, type ParsedSession } from './session';
import {
  laneMoveSpan,
  laneSpecFor,
  resetLaneFrom,
  spliceLaneEvents
} from '@renderer/utils/sessionEditOps';
import type { LaneMoveSpan, ResetExtent } from '@renderer/utils/sessionCore';
import { basename, indexByBasename } from '@renderer/utils/path';
import {
  normalizeGestureSamples,
  decimateSteps,
  relocateEventPaths,
  toggleFilterActiveRange,
  deleteFilterActiveSpan,
  resizeFilterActiveSpan,
  moveFilterActiveSpan,
  setRateAt,
  setRateSpan,
  moveTransportBlock,
  trimTransportBlock,
  splitTransportBlock,
  deleteTransportRanges,
  bmsVersion
} from '@renderer/utils/sessionCore';
import {
  TransportBlock,
  type EditableLaneKey,
  type Clip,
  type LanePoint,
  type SessionEvent
} from '@renderer/utils/types';

export type SelectedLane = { deck: string; lane: EditableLaneKey };

const MAX_UNDO = 100;

export const useSessionEditStore = defineStore('sessionEdit', () => {
  const sessionStore = useSessionStore();

  const editMode = ref(storageGet(STORAGE_KEYS.sessionEditMode, false));
  const selectedLane = ref<SelectedLane | null>(null);
  const undoStack = ref<SessionEvent[][]>([]);
  const redoStack = ref<SessionEvent[][]>([]);
  // The events array reference at load or last save. Splices always produce a
  // new array, so reference equality tells whether there are unsaved edits.
  const baseline = ref<SessionEvent[] | null>(null);

  let syncPromise: Promise<void> | null = null;

  const dirty = computed(
    () => sessionStore.session !== null && sessionStore.session.events !== baseline.value
  );
  const canUndo = computed(() => undoStack.value.length > 0);
  const canRedo = computed(() => redoStack.value.length > 0);

  watch(
    () => sessionStore.session,
    (current) => reset(current?.events ?? null)
  );

  function reset(baselineEvents: SessionEvent[] | null = sessionStore.session?.events ?? null) {
    selectedLane.value = null;
    undoStack.value = [];
    redoStack.value = [];
    baseline.value = baselineEvents;
    syncPromise = null;
  }

  function toggleEditMode() {
    editMode.value = !editMode.value;
    storageSet(STORAGE_KEYS.sessionEditMode, editMode.value);
    if (!editMode.value) selectedLane.value = null;
  }

  // Syncs are chained so rapid successive edits reach Rust in order. An older
  // payload must never overwrite a newer one.
  function syncToRust() {
    const session = sessionStore.session;
    if (!session) return;
    syncPromise = pushSessionEvents(syncPromise, session.path, JSON.stringify(session.events));
  }

  async function pushSessionEvents(
    previous: Promise<void> | null,
    path: string,
    eventsJson: string
  ): Promise<void> {
    if (previous) await previous;
    try {
      await call('update_session_events', { path, eventsJson });
    } catch (err) {
      console.error('[sessionEdit] failed to sync session events to Rust:', err);
    }
  }

  // Playback and render await this so they always run against the latest edit.
  async function flushSync(): Promise<void> {
    if (syncPromise) await syncPromise;
  }

  // A rejected edit returns its input unchanged, but every wrapper round-trips through
  // JSON, so the result is always a fresh array and a reference check alone would miss it.
  function isSameEdit(next: SessionEvent[], current: SessionEvent[]): boolean {
    if (next === current) return true;
    if (next.length !== current.length) return false;
    return JSON.stringify(next) === JSON.stringify(current);
  }

  function applyEdit(next: SessionEvent[]) {
    const session = sessionStore.session;
    if (!session || isSameEdit(next, session.events)) return;
    undoStack.value.push(session.events);
    if (undoStack.value.length > MAX_UNDO) undoStack.value.shift();
    redoStack.value = [];
    session.events = next;
    syncToRust();
  }

  async function commitGesture(
    deck: string,
    lane: EditableLaneKey,
    samples: LanePoint[],
    t0: number,
    t1: number,
    opts: { rateMin?: number; rateMax?: number } = {}
  ): Promise<void> {
    const session = sessionStore.session;
    if (!session || samples.length === 0) return;
    if (sessionStore.isPlaying) await sessionStore.stop();

    const spec = laneSpecFor(lane, session.mixerId, opts);
    const points = decimateSteps(normalizeGestureSamples(samples), spec.epsilon);
    applyEdit(spliceLaneEvents(session.events, spec, session.mixerId, deck, t0, t1, points));
  }

  async function commitLaneReset(
    deck: string,
    lane: EditableLaneKey,
    ms: number,
    extent: ResetExtent,
    opts: { rateMin?: number; rateMax?: number } = {}
  ): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    const spec = laneSpecFor(lane, session.mixerId, opts);
    applyEdit(resetLaneFrom(session.events, spec, session.mixerId, deck, ms, extent));
  }

  function moveSpanAt(
    deck: string,
    lane: EditableLaneKey,
    ms: number,
    opts: { rateMin?: number; rateMax?: number } = {}
  ): LaneMoveSpan | null {
    const session = sessionStore.session;
    if (!session) return null;
    return laneMoveSpan(
      session.events,
      laneSpecFor(lane, session.mixerId, opts),
      session.mixerId,
      deck,
      ms
    );
  }

  async function commitFilterActiveToggle(deck: string, t0: number, t1: number): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    applyEdit(toggleFilterActiveRange(session.events, deck, t0, t1));
  }

  // Like resize/move, deleting while playing stops playback first so the edit
  // applies cleanly against a settled timeline.
  async function deleteFilterSpan(deck: string, startMs: number, endMs: number): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    applyEdit(deleteFilterActiveSpan(session.events, deck, startMs, endMs));
  }

  async function resizeFilterSpan(
    deck: string,
    startMs: number,
    endMs: number,
    edge: 'start' | 'end',
    newMs: number
  ): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    applyEdit(
      resizeFilterActiveSpan(
        session.events,
        deck,
        startMs,
        endMs,
        edge,
        newMs,
        sessionStore.durationMs
      )
    );
  }

  async function moveFilterSpan(
    deck: string,
    startMs: number,
    endMs: number,
    deltaMs: number
  ): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    applyEdit(
      moveFilterActiveSpan(session.events, deck, startMs, endMs, deltaMs, sessionStore.durationMs)
    );
  }

  // Backs right-click "Set BPM from here".
  async function commitSetBpm(deck: string, ms: number, rate: number): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    applyEdit(setRateAt(session.events, deck, ms, rate));
  }

  // Backs right-click "Set BPM (whole clip)".
  async function commitSetClipBpm(
    deck: string,
    startMs: number,
    endMs: number,
    rate: number
  ): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    applyEdit(setRateSpan(session.events, deck, startMs, endMs, rate));
  }

  async function commitClipMove(
    clips: Clip[],
    block: TransportBlock,
    deltaMs: number
  ): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    applyEdit(moveTransportBlock(session.events, clips, block, deltaMs).events);
  }

  async function commitClipTrim(
    clips: Clip[],
    block: TransportBlock,
    edge: 'start' | 'end',
    newMs: number
  ): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    applyEdit(trimTransportBlock(session.events, clips, block, edge, newMs).events);
  }

  async function commitClipSplit(
    clips: Clip[],
    block: TransportBlock,
    splitMs: number
  ): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    applyEdit(splitTransportBlock(session.events, clips, block, splitMs));
  }

  async function commitRangesDelete(
    clips: Clip[],
    ranges: { deck: string; startMs: number; endMs: number }[]
  ): Promise<void> {
    const session = sessionStore.session;
    if (!session || ranges.length === 0) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    applyEdit(deleteTransportRanges(session.events, clips, ranges));
  }

  async function locateMissingTracks(): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const folder = await open({ directory: true, multiple: false });
    if (typeof folder !== 'string') return;
    if (sessionStore.isPlaying) await sessionStore.stop();

    const found = await call('scan_folder', { path: folder }).catch(() => [] as string[]);
    const byName = indexByBasename(found);

    // Identity mappings are skipped: a file found at the path the session
    // already records (it simply came back) is not an edit.
    const mapping: Record<string, string> = {};
    for (const missing of sessionStore.missingTracks) {
      const candidate = byName.get(basename(missing));
      if (candidate !== undefined && candidate !== missing) mapping[missing] = candidate;
    }

    const before = session.events;
    applyEdit(relocateEventPaths(session.events, mapping));
    // An unchanged path set never wakes the store's watcher, but the files may
    // exist again now.
    if (session.events === before) await sessionStore.checkMissingTracks();
  }

  function undo() {
    const session = sessionStore.session;
    if (!session) return;
    const previous = undoStack.value.pop();
    if (!previous) return;
    redoStack.value.push(session.events);
    session.events = previous;
    syncToRust();
  }

  function redo() {
    const session = sessionStore.session;
    if (!session) return;
    const next = redoStack.value.pop();
    if (!next) return;
    undoStack.value.push(session.events);
    session.events = next;
    syncToRust();
  }

  // Stamped rather than carried over from `raw`: loading ports the events, so a
  // session read as an older version is current by the time it can be saved.
  function serialize(session: ParsedSession): string {
    return JSON.stringify(
      { ...session.raw, version: bmsVersion(), events: session.events },
      null,
      2
    );
  }

  async function save(): Promise<boolean> {
    const session = sessionStore.session;
    if (!session) return false;
    try {
      await call('save_session', { path: session.path, content: serialize(session) });
    } catch {
      return false;
    }
    baseline.value = session.events;
    return true;
  }

  async function saveAs(): Promise<boolean> {
    const session = sessionStore.session;
    if (!session) return false;
    const baseName = session.filename.replace(/\.bms$/i, '');
    const path = await call('pick_save_path', { format: 'session', baseName });
    if (!path) return false;
    try {
      await call('save_session', { path, content: serialize(session) });
    } catch {
      return false;
    }
    session.path = path;
    session.filename = path.split('/').pop() ?? session.filename;
    // Re-key the Rust in-memory session under the new path so playback works.
    syncToRust();
    baseline.value = session.events;
    return true;
  }

  return {
    canRedo,
    canUndo,
    commitRangesDelete,
    commitClipMove,
    commitClipTrim,
    commitClipSplit,
    commitFilterActiveToggle,
    commitLaneReset,
    moveSpanAt,
    commitGesture,
    commitSetBpm,
    commitSetClipBpm,
    deleteFilterSpan,
    dirty,
    editMode,
    flushSync,
    locateMissingTracks,
    moveFilterSpan,
    redo,
    reset,
    resizeFilterSpan,
    save,
    saveAs,
    selectedLane,
    toggleEditMode,
    undo
  };
});
