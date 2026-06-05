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
  path: string;
};

export const useSessionStore = defineStore('session', () => {
  const session = ref<ParsedSession | null>(null);
  const isPlaying = ref(false);

  let autoStopTimeout: ReturnType<typeof setTimeout> | null = null;

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
      filename,
      path: file.path
    };

    invoke('preload_session', { path: file.path }).catch(() => {});

    return true;
  }

  async function play(fromMs = 0): Promise<void> {
    if (!session.value) return;

    isPlaying.value = true;

    if (autoStopTimeout != null) {
      clearTimeout(autoStopTimeout);
      autoStopTimeout = null;
    }

    try {
      await invoke('start_session_playback', { path: session.value.path, fromMs });
    } catch {
      isPlaying.value = false;
      return;
    }

    const remaining = session.value.durationMs - fromMs;
    if (remaining > 0) {
      autoStopTimeout = setTimeout(() => {
        isPlaying.value = false;
        autoStopTimeout = null;
      }, remaining);
    }
  }

  async function stop(): Promise<void> {
    isPlaying.value = false;
    if (autoStopTimeout != null) {
      clearTimeout(autoStopTimeout);
      autoStopTimeout = null;
    }
    await invoke('stop_session_playback');
  }

  async function ejectAllDecks(): Promise<void> {
    await Promise.all(['A', 'B', 'C', 'D'].map((deck) => invoke('stop', { deck })));
    await Promise.all(['A', 'B', 'C', 'D'].map((deck) => invoke('eject_track', { deck })));
  }

  async function exit(): Promise<void> {
    await stop();
    session.value = null;
    isPlaying.value = false;
  }

  return {
    session,
    isPlaying,
    durationMs,
    hasTrackInfo,
    openSession,
    play,
    stop,
    ejectAllDecks,
    exit
  };
});
