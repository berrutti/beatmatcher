import { ref, computed, watch } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { TrackWaveform, WaveformRegion } from '@renderer/utils/timelineDraw';
import { DECKS_DISPOSITION } from '@renderer/stores/decks';
import { DEFAULT_MIXER_ID } from '@renderer/stores/settings';
import type { SessionEvent } from '@renderer/utils/types';
import { portEvents } from '@renderer/utils/bmsCompatibility';
import { bmsVersion } from '@renderer/utils/sessionCore';

export type ParsedSession = {
  version: number;
  startedAt: string;
  // The mixer this session was played on. Lane ranges, labels and units come
  // from it, so it is not interchangeable with whatever the engine has loaded.
  mixerId: string;
  events: SessionEvent[];
  durationMs: number;
  filename: string;
  path: string;
  // Full JSON.parse result of the .bms file. Saving spreads this and overrides
  // `events`, so top-level fields the app does not model survive a round-trip.
  raw: Record<string, unknown>;
};

export type SessionLoadPhase = 'reading' | 'parsing' | 'decoding' | 'indexing' | 'done';

export type SessionLoadProgress = {
  path: string;
  phase: SessionLoadPhase;
  loadedBytes: number;
  totalBytes: number;
  loadedTracks: number;
  totalTracks: number;
  done: boolean;
};

// Two frames: the first only schedules a callback ahead of the paint that puts
// the modal on screen, so the caller must wait for the one after it.
function nextPaint(): Promise<void> {
  if (typeof requestAnimationFrame !== 'function') return Promise.resolve();
  return new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  });
}

export const useSessionStore = defineStore('session', () => {
  const session = ref<ParsedSession | null>(null);
  const isPlaying = ref(false);
  const loadProgress = ref<SessionLoadProgress | null>(null);
  // Per-track waveform region [startSec, endSec] at some point density. Zoom-
  // driven LOD (Timeline.vue) refetches a tighter range at higher density as the
  // user zooms in, so the visible span always renders near one point per pixel.
  const waveforms = ref(new Map<string, TrackWaveform>());
  const pendingWaveformPaths = new Set<string>();
  const pendingBasePaths = new Set<string>();
  // The latest region asked for while a fetch is in flight, so we converge to it
  // instead of dropping requests made mid-fetch.
  type RegionRequest = { startSec: number; endSec: number; numPoints: number };
  const waveformTarget = new Map<string, RegionRequest>();

  // True when the cached region already covers [startSec, endSec] at >= ~85% of
  // the requested point density (a little slack so tiny pans don't refetch).
  function regionSatisfies(region: WaveformRegion, req: RegionRequest): boolean {
    if (region.startSec > req.startSec + 1e-3 || region.endSec < req.endSec - 1e-3) return false;
    const cachedPps = region.amps.length / Math.max(1e-3, region.endSec - region.startSec);
    const neededPps = req.numPoints / Math.max(1e-3, req.endSec - req.startSec);
    return cachedPps >= neededPps * 0.85;
  }

  async function fetchRegion(req: RegionRequest, path: string): Promise<WaveformRegion> {
    const amps = await invoke<number[]>('get_track_amplitude_region', {
      path,
      startSec: req.startSec,
      endSec: req.endSec,
      numPoints: req.numPoints
    });
    return { startSec: req.startSec, endSec: req.endSec, amps: new Float32Array(amps) };
  }

  // The coarse, always-resident slice covering a track's whole used extent, so a
  // pan/zoom to an un-fetched spot still shows a low-detail texture immediately.
  // Loaded once per extent; the detailed region is layered on top.
  async function ensureWaveformBase(
    path: string,
    startSec: number,
    endSec: number,
    numPoints: number
  ): Promise<void> {
    const req: RegionRequest = { startSec, endSec, numPoints };
    const existing = waveforms.value.get(path);
    if (existing?.base && regionSatisfies(existing.base, req)) return;
    if (pendingBasePaths.has(path)) return;
    pendingBasePaths.add(path);
    try {
      const base = await fetchRegion(req, path);
      const prev = waveforms.value.get(path);
      const map = new Map(waveforms.value);
      // Keep the detail region if we already have one; otherwise show the base
      // as the detail too so the track renders before any zoom-in fetch.
      map.set(path, prev ? { ...prev, base } : { ...base, base });
      waveforms.value = map;
    } catch (err) {
      console.error('[session] failed to fetch base waveform region:', err);
    } finally {
      pendingBasePaths.delete(path);
    }
  }

  async function ensureWaveformRegion(
    path: string,
    startSec: number,
    endSec: number,
    numPoints: number
  ): Promise<void> {
    const req: RegionRequest = { startSec, endSec, numPoints };
    const cached = waveforms.value.get(path);
    if (cached && regionSatisfies(cached, req)) return;
    if (pendingWaveformPaths.has(path)) {
      waveformTarget.set(path, req);
      return;
    }
    pendingWaveformPaths.add(path);
    try {
      const region = await fetchRegion(req, path);
      const prev = waveforms.value.get(path);
      const map = new Map(waveforms.value);
      // Replace the detail region but keep the coarse base for fallback.
      map.set(path, { ...region, base: prev?.base });
      waveforms.value = map;
    } catch (err) {
      console.error('[session] failed to fetch waveform region:', err);
    } finally {
      pendingWaveformPaths.delete(path);
    }
    // Chase the most recent region requested while this fetch was in flight.
    const next = waveformTarget.get(path);
    if (next) {
      waveformTarget.delete(path);
      const now = waveforms.value.get(path);
      if (!now || !regionSatisfies(now, next)) {
        ensureWaveformRegion(path, next.startSec, next.endSec, next.numPoints).catch(() => {});
      }
    }
  }

  const trackPaths = computed(() => {
    const seen = new Set<string>();
    for (const event of session.value?.events ?? []) {
      if ((event.type === 'deck_snapshot' || event.type === 'load_track') && event.path) {
        seen.add(event.path);
      }
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
      missingTracks.value = paths.filter(
        (_, index) => sizes[index] === null || sizes[index] === undefined
      );
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

  // Audition-only mute/solo for session playback. Lives in the strip's mute
  // gain in Rust (independent of replayed fader events) and never affects
  // the offline render. Solo wins: when any deck is soloed, only soloed decks
  // are audible regardless of their mute state.
  const mutedDecks = ref<Set<string>>(new Set());
  const soloDecks = ref<Set<string>>(new Set());

  function deckAudible(deck: string): boolean {
    if (soloDecks.value.size > 0) return soloDecks.value.has(deck);
    return !mutedDecks.value.has(deck);
  }

  function applyAudibility() {
    for (const deck of DECKS_DISPOSITION) {
      invoke('set_deck_muted', { deck, muted: !deckAudible(deck) }).catch(() => {});
    }
  }

  // Mute and solo are mutually exclusive per deck: engaging one releases the
  // other, so a deck can never be in both lists.
  function toggleMute(deck: string) {
    const muted = new Set(mutedDecks.value);
    const solo = new Set(soloDecks.value);
    if (muted.has(deck)) {
      muted.delete(deck);
    } else {
      muted.add(deck);
      solo.delete(deck);
    }
    mutedDecks.value = muted;
    soloDecks.value = solo;
    applyAudibility();
  }

  function toggleSolo(deck: string) {
    const muted = new Set(mutedDecks.value);
    const solo = new Set(soloDecks.value);
    if (solo.has(deck)) {
      solo.delete(deck);
    } else {
      solo.add(deck);
      muted.delete(deck);
    }
    mutedDecks.value = muted;
    soloDecks.value = solo;
    applyAudibility();
  }

  function clearAudibility() {
    if (mutedDecks.value.size === 0 && soloDecks.value.size === 0) return;
    mutedDecks.value = new Set();
    soloDecks.value = new Set();
    applyAudibility();
  }

  const durationMs = computed(() => session.value?.durationMs ?? 0);
  const hasTrackInfo = computed(
    () =>
      session.value?.events.some((e) => e.type === 'deck_snapshot' || e.type === 'load_track') ??
      false
  );

  async function loadFromFile(path: string, content: string): Promise<boolean> {
    // Opened before the parse below, which blocks the main thread for seconds on
    // a long session: set after it, the window sits frozen with nothing on screen.
    loadProgress.value = {
      path,
      phase: 'parsing',
      loadedBytes: 0,
      totalBytes: 0,
      loadedTracks: 0,
      totalTracks: 0,
      done: false
    };
    await nextPaint();

    let raw: {
      version: number;
      startedAt: string;
      mixer?: { id?: string };
      events: SessionEvent[];
    };
    try {
      raw = JSON.parse(content);
    } catch {
      loadProgress.value = null;
      return false;
    }

    const events: SessionEvent[] = portEvents(raw.events ?? [], raw.version);
    // Max, not last: a .bms with sub-ms ordering drift is not strictly sorted.
    let durationMs = 0;
    for (const event of events) {
      durationMs = Math.max(durationMs, event.elapsed_ms);
    }
    const parts = path.split('/');
    const filename = parts[parts.length - 1] ?? 'session.bms';

    session.value = {
      version: bmsVersion(),
      startedAt: raw.startedAt ?? '',
      // Sessions written before manifests existed have no header, and every one
      // of those was played on the classic mixer.
      mixerId: raw.mixer?.id ?? DEFAULT_MIXER_ID,
      events,
      durationMs,
      filename,
      path,
      raw: raw as unknown as Record<string, unknown>
    };

    invoke('preload_session', { path }).catch(() => {
      if (loadProgress.value?.path !== path) return;
      loadProgress.value = null;
    });

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

  listen<SessionLoadProgress>('session-load-progress', (event) => {
    if (event.payload.path !== session.value?.path) return;
    loadProgress.value = event.payload;
  }).catch(() => {});

  // Every track has to be decoded before the scheduler can place it, so playback is refused
  // over queued: a press that silently starts seconds later reads as a broken button.
  const isLoading = computed(() => loadProgress.value !== null && !loadProgress.value.done);

  const loadedFraction = computed(() => {
    const progress = loadProgress.value;
    if (!progress) return 1;
    if (progress.done) return 1;
    if (progress.totalBytes > 0) return progress.loadedBytes / progress.totalBytes;
    if (progress.totalTracks > 0) return progress.loadedTracks / progress.totalTracks;
    return 0;
  });

  async function play(fromMs = 0): Promise<void> {
    if (!session.value || isLoading.value) return;
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
    clearAudibility();
    const path = session.value?.path;
    session.value = null;
    loadProgress.value = null;
    waveforms.value = new Map();
    waveformTarget.clear();
    if (path) await invoke('unload_session', { path }).catch(() => {});
  }

  async function exit(): Promise<void> {
    await unload();
  }

  return {
    session,
    isPlaying,
    loadProgress,
    isLoading,
    loadedFraction,
    waveforms,
    durationMs,
    hasTrackInfo,
    missingTracks,
    checkMissingTracks,
    mutedDecks,
    soloDecks,
    deckAudible,
    toggleMute,
    toggleSolo,
    ensureWaveformRegion,
    ensureWaveformBase,
    openSession,
    openSessionFromPath,
    play,
    stop,
    unload,
    exit
  };
});
