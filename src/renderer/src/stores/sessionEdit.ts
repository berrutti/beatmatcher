import { ref, computed, watch } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import { useSessionStore, type ParsedSession } from './session';
import { useSettingsStore } from './settings';
import { laneSpecFor, spliceLaneEvents } from '@renderer/utils/sessionEditOps';
import { basename, indexByBasename } from '@renderer/utils/path';
import {
  normalizeGestureSamples,
  decimateSteps,
  deleteNudgeRange,
  relocateEventPaths,
  toggleFilterActiveRange,
  deleteFilterActiveSpan,
  resizeFilterActiveSpan,
  moveFilterActiveSpan,
  paintNudgeRange,
  setRateAt,
  setRateSpan,
  moveTransportBlock,
  trimTransportBlock,
  deleteTransportRanges
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

  const editMode = ref(false);
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
    editMode.value = false;
    selectedLane.value = null;
    undoStack.value = [];
    redoStack.value = [];
    baseline.value = baselineEvents;
    syncPromise = null;
  }

  function toggleEditMode() {
    editMode.value = !editMode.value;
    if (!editMode.value) selectedLane.value = null;
  }

  // Syncs are chained so rapid successive edits reach Rust in order; an older
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
      await invoke('update_session_events', { path, eventsJson });
    } catch (err) {
      console.error('[sessionEdit] failed to sync session events to Rust:', err);
    }
  }

  // Playback and render await this so they always run against the latest edit.
  async function flushSync(): Promise<void> {
    if (syncPromise) await syncPromise;
  }

  function applyEdit(next: SessionEvent[]) {
    const session = sessionStore.session;
    if (!session || next === session.events) return;
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

    const spec = laneSpecFor(lane, opts);
    const points = decimateSteps(normalizeGestureSamples(samples), spec.epsilon);
    applyEdit(spliceLaneEvents(session.events, spec, deck, t0, t1, points));
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

  async function commitNudgePaint(
    deck: string,
    t0: number,
    t1: number,
    direction: 1 | -1
  ): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    const percent = direction * useSettingsStore().nudgeSensitivity;
    applyEdit(paintNudgeRange(session.events, deck, t0, t1, percent));
  }

  // Right-click "Set BPM from here": insert one rate change at `atMs`. The
  // caller converts the entered BPM to rate (target / clip track bpm); the new
  // value holds until the next existing change, splitting the clip into a new
  // wave segment (the timeline already stretches the waveform per segment).
  async function commitSetBpm(deck: string, atMs: number, rate: number): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    applyEdit(setRateAt(session.events, deck, atMs, rate));
  }

  // Right-click "Set BPM (whole clip)": one uniform rate over [startMs, endMs],
  // dropping any rate changes inside the clip and restoring the prior rate after,
  // so the clip plays at one tempo without adding a new region.
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

  async function deleteNudge(deck: string, t0: number, t1: number): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    applyEdit(deleteNudgeRange(session.events, deck, t0, t1));
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

  async function commitRangesDelete(
    clips: Clip[],
    ranges: { deck: string; startMs: number; endMs: number }[]
  ): Promise<void> {
    const session = sessionStore.session;
    if (!session || ranges.length === 0) return;
    if (sessionStore.isPlaying) await sessionStore.stop();
    applyEdit(deleteTransportRanges(session.events, clips, ranges));
  }

  // Opens a folder picker and resolves every missing track found under it
  // (recursively, matched by filename), so one pick relinks a whole moved
  // library. Goes through applyEdit, so the relocation is undoable, marks the
  // session dirty, and syncs to Rust; saving afterwards persists the new
  // paths in the .bms.
  async function locateMissingTracks(): Promise<void> {
    const session = sessionStore.session;
    if (!session) return;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const folder = await open({ directory: true, multiple: false });
    if (typeof folder !== 'string') return;
    if (sessionStore.isPlaying) await sessionStore.stop();

    const found = await invoke<string[]>('scan_folder', { path: folder }).catch(
      () => [] as string[]
    );
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
    // When nothing was rewritten the path set is unchanged and the watcher in
    // the session store never fires, but the files may exist again now. When
    // an edit did happen, the watcher already triggers the recheck.
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

  function serialize(session: ParsedSession): string {
    return JSON.stringify({ ...session.raw, events: session.events }, null, 2);
  }

  async function save(): Promise<boolean> {
    const session = sessionStore.session;
    if (!session) return false;
    try {
      await invoke('save_session', { path: session.path, content: serialize(session) });
    } catch {
      return false;
    }
    baseline.value = session.events;
    return true;
  }

  async function saveAs(): Promise<boolean> {
    const session = sessionStore.session;
    if (!session) return false;
    const path = await invoke<string | null>('pick_save_path', { format: 'session' });
    if (!path) return false;
    try {
      await invoke('save_session', { path, content: serialize(session) });
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
    commitFilterActiveToggle,
    commitGesture,
    commitNudgePaint,
    commitSetBpm,
    commitSetClipBpm,
    deleteFilterSpan,
    deleteNudge,
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
