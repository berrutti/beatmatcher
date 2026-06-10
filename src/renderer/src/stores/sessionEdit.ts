import { ref, computed, watch } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import { useSessionStore, type SessionEvent, type ParsedSession } from './session';
import { useSettingsStore } from './settings';
import type { LanePoint } from '@renderer/composables/useSessionTimeline';
import {
  laneSpecFor,
  normalizeGestureSamples,
  decimateSteps,
  spliceLaneEvents,
  toggleFilterActiveRange,
  paintNudgeRange,
  type EditableLaneKey
} from '@renderer/utils/sessionEditOps';

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
    } catch {
      // ignored: the next sync or playback attempt surfaces the failure
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
    editMode,
    selectedLane,
    dirty,
    canUndo,
    canRedo,
    toggleEditMode,
    commitGesture,
    commitFilterActiveToggle,
    commitNudgePaint,
    flushSync,
    undo,
    redo,
    save,
    saveAs,
    reset
  };
});
