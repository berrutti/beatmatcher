import { describe, it, expect, vi, beforeEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import { nextTick } from 'vue';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(null)
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {})
}));

vi.mock('@tauri-apps/plugin-store', () => ({
  load: vi.fn().mockResolvedValue({})
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn()
}));

vi.mock('@renderer/stores/settings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@renderer/stores/settings')>();
  return {
    ...actual,
    useSettingsStore: () => ({ nudgeSensitivity: 4 })
  };
});

import { useSessionStore } from '../session';
import { useSessionEditStore } from '../sessionEdit';
import { invoke } from '@tauri-apps/api/core';
import { DEFAULT_MIXER_ID } from '../settings';
import type { SessionEvent } from '@renderer/utils/types';
import { DECK_LANE_KEYS } from '@renderer/utils/types';

// Mirrors MAX_UNDO in sessionEdit.ts.
const MAX_UNDO = 100;

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

function baseEvents(): SessionEvent[] {
  return [
    { elapsed_ms: 0, type: 'recording_start' },
    { elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/tracks/one.mp3' },
    { elapsed_ms: 500, type: 'play', deck: 'A' },
    { elapsed_ms: 60_000, type: 'stop', deck: 'A' },
    { elapsed_ms: 60_100, type: 'recording_stop' }
  ];
}

// Edit store first, as `App.vue` does: its watcher captures the dirty baseline.
async function loadSession(events: SessionEvent[]) {
  const editStore = useSessionEditStore();
  const store = useSessionStore();
  store.session = {
    version: 2,
    startedAt: '2026-06-11T00:00:00Z',
    mixerId: DEFAULT_MIXER_ID,
    events,
    durationMs: 60_100,
    filename: 'mix.bms',
    path: '/sessions/mix.bms',
    raw: {}
  };
  await nextTick();
  return { store, editStore };
}

// Long enough to clear MIN_GESTURE_MS, so it is an edit rather than a rejection.
async function randomGesture(
  editStore: ReturnType<typeof useSessionEditStore>,
  random: () => number
): Promise<void> {
  const lane = DECK_LANE_KEYS[Math.floor(random() * DECK_LANE_KEYS.length)];
  const t0 = Math.floor(random() * 50_000);
  const t1 = t0 + 500 + Math.floor(random() * 5000);
  const samples = [
    { ms: t0, value: random() },
    { ms: (t0 + t1) / 2, value: random() },
    { ms: t1, value: random() }
  ];
  await editStore.commitGesture('A', lane, samples, t0, t1);
}

describe('undo and redo under fuzzed edit sequences', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    vi.mocked(invoke).mockResolvedValue(null);
  });

  it('every undo is reversible by the redo that follows it', async () => {
    for (let seed = 1; seed <= 40; seed++) {
      setActivePinia(createPinia());
      const random = makeRandom(seed);
      const { store, editStore } = await loadSession(baseEvents());

      for (let edit = 0; edit < 4; edit++) await randomGesture(editStore, random);

      const steps = 1 + Math.floor(random() * 4);
      for (let step = 0; step < steps; step++) {
        if (!editStore.canUndo) break;
        const before = store.session?.events;
        editStore.undo();
        expect(editStore.canRedo, `seed ${seed}`).toBe(true);
        editStore.redo();
        expect(store.session?.events, `seed ${seed} step ${step}`).toBe(before);
      }
    }
  });

  it('undoing everything returns the exact array the session was loaded with', async () => {
    for (let seed = 1; seed <= 40; seed++) {
      setActivePinia(createPinia());
      const random = makeRandom(seed * 7);
      const { store, editStore } = await loadSession(baseEvents());
      // Read back through the store: the events array is reactive, so the
      // baseline the store compares against is its proxy, not the raw array.
      const loaded = store.session?.events;

      const edits = 1 + Math.floor(random() * 6);
      for (let edit = 0; edit < edits; edit++) await randomGesture(editStore, random);
      expect(editStore.dirty, `seed ${seed}`).toBe(true);

      let guard = 0;
      while (editStore.canUndo && guard++ < MAX_UNDO * 2) editStore.undo();

      expect(store.session?.events, `seed ${seed}`).toBe(loaded);
      // Reference-identical to the baseline, so nothing is left to save.
      expect(editStore.dirty, `seed ${seed}`).toBe(false);
    }
  });

  it('an edit made after an undo drops the redo branch', async () => {
    for (let seed = 1; seed <= 40; seed++) {
      setActivePinia(createPinia());
      const random = makeRandom(seed * 13);
      const { editStore } = await loadSession(baseEvents());

      await randomGesture(editStore, random);
      await randomGesture(editStore, random);
      editStore.undo();
      expect(editStore.canRedo, `seed ${seed}`).toBe(true);

      await randomGesture(editStore, random);

      expect(editStore.canRedo, `seed ${seed}`).toBe(false);
    }
  });

  it('keeps at most one undo step per edit and never more than the cap', async () => {
    const random = makeRandom(99);
    const { store, editStore } = await loadSession(baseEvents());

    const edits = MAX_UNDO + 25;
    for (let edit = 0; edit < edits; edit++) await randomGesture(editStore, random);

    let depth = 0;
    while (editStore.canUndo) {
      editStore.undo();
      depth++;
      expect(depth).toBeLessThanOrEqual(MAX_UNDO);
    }
    expect(depth).toBe(MAX_UNDO);
    // Past the cap the loaded array is gone, so the session cannot come back clean.
    expect(store.session?.events).not.toBe(null);
    expect(editStore.dirty).toBe(true);
  });

  it('never leaves an event without a finite timestamp or a type', async () => {
    for (let seed = 1; seed <= 40; seed++) {
      setActivePinia(createPinia());
      const random = makeRandom(seed * 31);
      const { store, editStore } = await loadSession(baseEvents());

      for (let edit = 0; edit < 5; edit++) {
        await randomGesture(editStore, random);
        if (random() < 0.3 && editStore.canUndo) editStore.undo();
        if (random() < 0.2 && editStore.canRedo) editStore.redo();
      }

      for (const event of store.session?.events ?? []) {
        expect(Number.isFinite(event.elapsed_ms), `seed ${seed}`).toBe(true);
        expect(typeof event.type, `seed ${seed}`).toBe('string');
        expect(event.type.length, `seed ${seed}`).toBeGreaterThan(0);
      }
    }
  });
});
