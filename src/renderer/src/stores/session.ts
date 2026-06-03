import { ref, computed } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';

type SessionEvent = {
  elapsed_ms: number;
  type: string;
  deck?: string;
  path?: string;
  sec?: number;
  gain?: number;
  band?: string;
  db?: number;
  value?: number;
  active?: boolean;
  rate?: number;
  percent?: number;
  quantized?: boolean;
  beat_offset_sec?: number;
  start_sec?: number;
  end_sec?: number;
  is_playing?: boolean;
  position_sec?: number;
  cue_point_sec?: number;
  loop_active?: boolean;
  loop_start_sec?: number;
  loop_end_sec?: number;
  bpm?: number;
  playback_rate?: number;
  duration?: number;
};

type ParsedSession = {
  version: number;
  startedAt: string;
  events: SessionEvent[];
  durationMs: number;
  filename: string;
};

const SKIP_TYPES = new Set([
  'recording_start',
  'recording_stop',
  'deck_snapshot',
  'cue_move',
  'set_cue_active'
]);

export const useSessionStore = defineStore('session', () => {
  const sessionMode = ref(false);
  const session = ref<ParsedSession | null>(null);
  const isPlaying = ref(false);

  let timeouts: ReturnType<typeof setTimeout>[] = [];

  const durationMs = computed(() => session.value?.durationMs ?? 0);
  const hasTrackInfo = computed(
    () =>
      session.value?.events.some((e) => e.type === 'deck_snapshot' || e.type === 'load_track') ??
      false
  );

  async function openSession(): Promise<boolean> {
    const file = await invoke<{ path: string; content: string } | null>('open_session_dialog');
    if (!file) return false;

    let raw: { version: number; startedAt: string; events: SessionEvent[] };
    try {
      raw = JSON.parse(file.content);
    } catch {
      return false;
    }

    const events: SessionEvent[] = raw.events ?? [];
    const lastEvent = events[events.length - 1];
    const durationMs = lastEvent?.elapsed_ms ?? 0;
    const parts = file.path.split('/');
    const filename = parts[parts.length - 1] ?? 'session.json';

    session.value = {
      version: raw.version ?? 1,
      startedAt: raw.startedAt ?? '',
      events,
      durationMs,
      filename
    };
    return true;
  }

  async function play(): Promise<void> {
    if (!session.value) return;

    // Clear any previous playback before starting fresh.
    timeouts.forEach(clearTimeout);
    timeouts = [];
    await Promise.all(['A', 'B', 'C', 'D'].map((deck) => invoke('stop', { deck })));
    await Promise.all(['A', 'B', 'C', 'D'].map((deck) => invoke('eject_track', { deck })));
    isPlaying.value = false;

    const snapshots = session.value.events.filter((e) => e.type === 'deck_snapshot');
    const snapshotPlays: Array<{ deck: string; fromSec: number }> = [];

    if (snapshots.length > 0) {
      await Promise.all(
        snapshots.map(async (snap) => {
          if (!snap.deck || !snap.path) return;
          await invoke('load_track', { deck: snap.deck, path: snap.path, analyze: false, beatOffsetSec: 0 });
          // Always seek — never play yet. Playing decks are collected and
          // scheduled at elapsed_ms=0 together with all other events so
          // they share the same time reference and don't drift.
          if (snap.position_sec != null) {
            await invoke('seek', { deck: snap.deck, sec: snap.position_sec });
          }
          if (snap.is_playing) {
            snapshotPlays.push({ deck: snap.deck, fromSec: snap.position_sec ?? 0 });
          }
          if (snap.playback_rate != null && snap.playback_rate !== 1) {
            await invoke('set_playback_rate', { deck: snap.deck, rate: snap.playback_rate });
          }
        })
      );
    }

    isPlaying.value = true;

    // Start decks that were playing at recording time at t=0, same reference
    // as every other scheduled event.
    for (const { deck, fromSec } of snapshotPlays) {
      const id = setTimeout(() => invoke('play', { deck, fromSec }), 0);
      timeouts.push(id);
    }

    for (const event of session.value.events) {
      if (SKIP_TYPES.has(event.type)) continue;
      const delay = event.elapsed_ms;
      if (delay < 0) continue;
      const id = setTimeout(() => executeEvent(event), delay);
      timeouts.push(id);
    }

    const totalMs = session.value.durationMs;
    if (totalMs > 0) {
      const endId = setTimeout(() => { isPlaying.value = false; }, totalMs);
      timeouts.push(endId);
    }
  }

  async function executeEvent(event: SessionEvent): Promise<void> {
    const { type, deck } = event;
    switch (type) {
      case 'cue_preview_start':
        if (deck && event.cue_point_sec != null)
          await invoke('play', { deck, fromSec: event.cue_point_sec });
        break;
      case 'cue_preview_end':
        if (deck) {
          await invoke('stop', { deck });
          if (event.cue_point_sec != null) await invoke('seek', { deck, sec: event.cue_point_sec });
        }
        break;
      case 'play':
        if (deck) await invoke('play', { deck });
        break;
      case 'stop':
        if (deck) await invoke('stop', { deck });
        break;
      case 'stopped_at_cue':
      case 'stop_at_cue':
        if (deck) {
          await invoke('stop', { deck });
          if (event.cue_point_sec != null) await invoke('seek', { deck, sec: event.cue_point_sec });
        }
        break;
      case 'seek':
        if (deck && event.sec != null) await invoke('seek', { deck, sec: event.sec });
        break;
      case 'load_track':
        if (deck && event.path)
          await invoke('load_track', { deck, path: event.path, analyze: false, beatOffsetSec: 0 });
        break;
      case 'eject_track':
        if (deck) await invoke('eject_track', { deck });
        break;
      case 'set_volume':
        if (deck && event.gain != null) await invoke('set_volume', { deck, gain: event.gain });
        break;
      case 'set_eq':
        if (deck && event.band && event.db != null)
          await invoke('set_eq', { deck, band: event.band, db: event.db });
        break;
      case 'set_filter':
        if (deck && event.value != null) await invoke('set_filter', { deck, value: event.value });
        break;
      case 'set_filter_active':
        if (deck && event.active != null)
          await invoke('set_filter_active', { deck, active: event.active });
        break;
      case 'set_playback_rate':
        if (deck && event.rate != null)
          await invoke('set_playback_rate', { deck, rate: event.rate });
        break;
      case 'set_nudge':
        if (deck && event.percent != null)
          await invoke('set_nudge', { deck, percent: event.percent });
        break;
      case 'set_master_gain':
        if (event.gain != null) await invoke('set_master_gain', { gain: event.gain });
        break;
      case 'set_beat_grid':
        if (deck && event.bpm != null && event.beat_offset_sec != null)
          await invoke('set_beat_grid', {
            deck,
            bpm: event.bpm,
            beatOffsetSec: event.beat_offset_sec
          });
        break;
      case 'loop_in':
        // set_loop_in no longer accepts params — replay by setting region directly.
        // cue_sec from the event is the exact (possibly quantized) value that was recorded.
        if (deck && event.cue_sec != null) {
          await invoke('set_loop_region', { deck, startSec: event.cue_sec, endSec: 0 });
          await invoke('set_loop_active', { deck, active: false });
        }
        break;
      case 'loop_out':
        // set_loop_out no longer accepts params — replay using the exact recorded values.
        if (deck && event.start_sec != null && event.end_sec != null) {
          await invoke('set_loop_region', { deck, startSec: event.start_sec, endSec: event.end_sec });
          await invoke('set_loop_active', { deck, active: true });
        }
        break;
      case 'reloop':
        if (deck) await invoke('set_reloop', { deck });
        break;
      case 'exit_loop':
        if (deck) await invoke('set_loop_active', { deck, active: false });
        break;
    }
  }

  async function stop(): Promise<void> {
    isPlaying.value = false;
    timeouts.forEach(clearTimeout);
    timeouts = [];
    await Promise.all(['A', 'B', 'C', 'D'].map((deck) => invoke('stop', { deck })));
  }

  async function stopAllDecks(): Promise<void> {
    await Promise.all(['A', 'B', 'C', 'D'].map((deck) => invoke('stop', { deck })));
  }

  async function ejectAllDecks(): Promise<void> {
    await Promise.all(['A', 'B', 'C', 'D'].map((deck) => invoke('stop', { deck })));
    await Promise.all(['A', 'B', 'C', 'D'].map((deck) => invoke('eject_track', { deck })));
  }

  function enter(): void {
    sessionMode.value = true;
  }

  async function exit(): Promise<void> {
    await stop();
    sessionMode.value = false;
    session.value = null;
    isPlaying.value = false;
  }

  return {
    sessionMode,
    session,
    isPlaying,
    durationMs,
    hasTrackInfo,
    openSession,
    play,
    stop,
    stopAllDecks,
    ejectAllDecks,
    enter,
    exit
  };
});
