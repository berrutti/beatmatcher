import { ref, computed, watch } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import { call } from '@renderer/tauriCommands';
import { listen } from '@tauri-apps/api/event';
import type { TrackWaveform, WaveformRegion } from '@renderer/utils/timelineDraw';
import { DECKS_DISPOSITION } from '@renderer/stores/decks';
import { DEFAULT_MIXER_ID, useSettingsStore } from '@renderer/stores/settings';
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
export type RenderProgress = { fraction: number; writing: boolean };

export const SESSION_LOAD_PHASE_KEYS: Record<SessionLoadPhase, string> = {
  reading: 'session.loadingPhaseReading',
  parsing: 'session.loadingPhaseParsing',
  decoding: 'session.loadingPhaseDecoding',
  indexing: 'session.loadingPhaseIndexing',
  done: 'session.loadingPhaseIndexing'
};

// Only the decode reports increments. Reading, parsing and indexing each take
// one long step, so a percentage there would sit at 0 and read as a hang.
export function sessionLoadIsMeasured(phase: SessionLoadPhase): boolean {
  return phase === 'decoding' || phase === 'done';
}

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
  // Null except while a render is in flight, so a stray event from a finished
  // render cannot reopen the modal.
  const renderProgress = ref<RenderProgress | null>(null);
  // Refetched tighter as the user zooms, so the visible span stays near one
  // point per pixel.
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
    const amps = await call('get_track_amplitude_region', {
      path,
      startSec: req.startSec,
      endSec: req.endSec,
      numPoints: req.numPoints
    });
    return { startSec: req.startSec, endSec: req.endSec, amps: new Float32Array(amps) };
  }

  // Always resident, so a pan to an unfetched spot shows something immediately.
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
      map.set(path, { ...region, base: prev?.base });
      waveforms.value = map;
    } catch (err) {
      console.error('[session] failed to fetch waveform region:', err);
    } finally {
      pendingWaveformPaths.delete(path);
    }
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
      const sizes = await call('files_info', { paths });
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

  // Audition only: it rides the strip's mute gain in Rust, apart from the
  // replayed fader events, and the offline render ignores it.
  // Off rather than on, so a session with nothing set leaves every deck enabled.
  const disabledDecks = ref<Set<string>>(new Set());
  // One at a time: soloing a deck releases whichever was soloed before.
  const soloedDeck = ref<string | null>(null);

  function deckEnabled(deck: string): boolean {
    return !disabledDecks.value.has(deck);
  }

  // A solo overrides the switches entirely, the soloed deck's own included: it
  // plays whether or not it is enabled, and nothing else plays either way.
  function deckAudible(deck: string): boolean {
    if (soloedDeck.value !== null) return deck === soloedDeck.value;
    return deckEnabled(deck);
  }

  function applyAudibility() {
    for (const deck of DECKS_DISPOSITION) {
      call('set_deck_muted', { deck, muted: !deckAudible(deck) }).catch(() => {});
    }
  }

  // Still recorded while a solo is up, so dropping the solo restores whatever the
  // switches were left at rather than turning everything back on.
  function toggleDeckEnabled(deck: string) {
    const next = new Set(disabledDecks.value);
    if (!next.delete(deck)) next.add(deck);
    disabledDecks.value = next;
    applyAudibility();
  }

  function toggleSolo(deck: string) {
    soloedDeck.value = soloedDeck.value === deck ? null : deck;
    applyAudibility();
  }

  function clearAudibility() {
    if (disabledDecks.value.size === 0 && soloedDeck.value === null) return;
    disabledDecks.value = new Set();
    soloedDeck.value = null;
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

    call('preload_session', { path }).catch(() => {
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
    const content = await call('read_file', { path }).catch(() => null);
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

  listen<RenderProgress>('render-progress', (event) => {
    if (renderProgress.value) renderProgress.value = event.payload;
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
      await call('start_session_playback', { path: session.value.path, fromMs });
    } catch {
      isPlaying.value = false;
    }
  }

  async function stop(): Promise<void> {
    isPlaying.value = false;
    await call('stop_session_playback');
  }

  async function unload(): Promise<void> {
    if (isPlaying.value) await stop();
    clearAudibility();
    const path = session.value?.path;
    session.value = null;
    loadProgress.value = null;
    waveforms.value = new Map();
    waveformTarget.clear();
    if (path) await call('unload_session', { path }).catch(() => {});
  }

  async function exit(): Promise<void> {
    await unload();
  }

  async function pickRenderOutputPath(useFlac: boolean, baseName: string): Promise<string | null> {
    return call('pick_save_path', { format: useFlac ? 'flac' : 'wav', baseName });
  }

  async function renderSession(
    sessionPath: string,
    outputPath: string,
    useFlac: boolean
  ): Promise<void> {
    renderProgress.value = { fraction: 0, writing: false };
    try {
      await call('render_session_to_file', {
        sessionPath,
        outputPath,
        useFlac,
        writeCue: useSettingsStore().recordCue
      });
    } finally {
      renderProgress.value = null;
    }
  }

  // The encode cannot be interrupted, so the button is withdrawn once it starts
  // rather than accepting a press that would do nothing.
  async function cancelRender(): Promise<void> {
    await call('cancel_render');
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
    disabledDecks,
    soloedDeck,
    deckEnabled,
    deckAudible,
    toggleDeckEnabled,
    toggleSolo,
    ensureWaveformRegion,
    ensureWaveformBase,
    openSession,
    openSessionFromPath,
    renderProgress,
    renderSession,
    cancelRender,
    pickRenderOutputPath,
    play,
    stop,
    unload,
    exit
  };
});
