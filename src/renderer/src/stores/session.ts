import { ref, computed, watch } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { TrackWaveform } from '@renderer/utils/timelineDraw';

export type SessionEvent = {
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
  loop_end_sec?: number;
  bpm?: number;
  playback_rate?: number;
  duration?: number;
};

export type ParsedSession = {
  version: number;
  startedAt: string;
  events: SessionEvent[];
  durationMs: number;
  filename: string;
  path: string;
  // Full JSON.parse result of the .bms file. Saving spreads this and overrides
  // `events`, so top-level fields the app does not model survive a round-trip.
  raw: Record<string, unknown>;
};

export const useSessionStore = defineStore('session', () => {
  const session = ref<ParsedSession | null>(null);
  const isPlaying = ref(false);
  const waveforms = ref(new Map<string, TrackWaveform>());
  const pendingWaveformPaths = new Set<string>();

  async function ensureWaveform(path: string, numPoints = 500): Promise<void> {
    if (waveforms.value.has(path) || pendingWaveformPaths.has(path)) return;
    pendingWaveformPaths.add(path);
    try {
      const result = await invoke<{ durationSec: number; amps: number[] }>(
        'get_track_amplitude_waveform',
        { path, numPoints }
      );
      const map = new Map(waveforms.value);
      map.set(path, { durationSec: result.durationSec, amps: new Float32Array(result.amps) });
      waveforms.value = map;
    } catch {
      // ignore fetch failures
    } finally {
      pendingWaveformPaths.delete(path);
    }
  }

  const trackPaths = computed(() => {
    const seen = new Set<string>();
    for (const e of session.value?.events ?? []) {
      if ((e.type === 'deck_snapshot' || e.type === 'load_track') && e.path) seen.add(e.path);
    }
    return [...seen];
  });

  const missingTracks = ref<string[]>([]);

  async function checkMissingTracks(): Promise<void> {
    const paths = trackPaths.value;
    if (paths.length === 0) {
      missingTracks.value = [];
      return;
    }
    try {
      const sizes = await invoke<(number | null)[]>('files_info', { paths });
      missingTracks.value = paths.filter((_, i) => sizes[i] === null || sizes[i] === undefined);
    } catch {
      missingTracks.value = [];
    }
  }

  // Recheck whenever the set of referenced files changes (session opened or a
  // missing file relocated), not on every event edit.
  watch(
    () => trackPaths.value.join('\n'),
    () => {
      checkMissingTracks().catch(() => {});
    }
  );

  const durationMs = computed(() => session.value?.durationMs ?? 0);
  const hasTrackInfo = computed(
    () =>
      session.value?.events.some((e) => e.type === 'deck_snapshot' || e.type === 'load_track') ??
      false
  );

  async function loadFromFile(path: string, content: string): Promise<boolean> {
    let raw: { version: number; startedAt: string; events: SessionEvent[] };
    try {
      raw = JSON.parse(content);
    } catch {
      return false;
    }

    const events: SessionEvent[] = raw.events ?? [];
    const lastEvent = events[events.length - 1];
    const durationMs = lastEvent?.elapsed_ms ?? 0;
    const parts = path.split('/');
    const filename = parts[parts.length - 1] ?? 'session.bms';

    session.value = {
      version: raw.version ?? 1,
      startedAt: raw.startedAt ?? '',
      events,
      durationMs,
      filename,
      path,
      raw: raw as unknown as Record<string, unknown>
    };

    invoke('preload_session', { path }).catch(() => {});

    return true;
  }

  async function openSession(): Promise<boolean> {
    const file = await invoke<{ path: string; content: string } | null>('open_session_dialog');
    if (!file) return false;
    return loadFromFile(file.path, file.content);
  }

  async function openSessionFromPath(path: string): Promise<boolean> {
    const content = await invoke<string>('read_file', { path }).catch(() => null);
    if (!content) return openSession();
    return loadFromFile(path, content);
  }

  listen('session-playback-ended', () => {
    isPlaying.value = false;
  }).catch(() => {});

  async function play(fromMs = 0): Promise<void> {
    if (!session.value) return;
    isPlaying.value = true;
    try {
      await invoke('start_session_playback', { path: session.value.path, fromMs });
    } catch {
      isPlaying.value = false;
    }
  }

  async function stop(): Promise<void> {
    isPlaying.value = false;
    await invoke('stop_session_playback');
  }

  async function unload(): Promise<void> {
    if (isPlaying.value) await stop();
    const path = session.value?.path;
    session.value = null;
    waveforms.value = new Map();
    if (path) await invoke('unload_session', { path }).catch(() => {});
  }

  async function exit(): Promise<void> {
    await unload();
  }

  return {
    session,
    isPlaying,
    waveforms,
    durationMs,
    hasTrackInfo,
    missingTracks,
    checkMissingTracks,
    ensureWaveform,
    openSession,
    openSessionFromPath,
    play,
    stop,
    unload,
    exit
  };
});
