import { computed, type Ref } from 'vue';

type SessionEvent = {
  elapsed_ms: number;
  type: string;
  deck?: string;
  path?: string;
  sec?: number;
  rate?: number;
  is_playing?: boolean;
  position_sec?: number;
  cue_point_sec?: number;
  playback_rate?: number;
  active?: boolean;
  loop_active?: boolean;
  loop_start_sec?: number;
  loop_end_sec?: number;
  start_sec?: number;
  end_sec?: number;
};

export type Clip = {
  deck: string;
  sessionStartMs: number;
  sessionEndMs: number;
  trackPath: string;
  trackName: string;
  trackStartSec: number;
  playbackRate: number;
};

export type LoadedSpan = {
  deck: string;
  trackPath: string;
  trackName: string;
  startMs: number;
  endMs: number;
};

type DeckState = {
  path: string | null;
  trackPosSec: number;
  rate: number;
  loopStartSec: number | null;
  loopEndSec: number | null;
  loopActive: boolean;
  loopEngagedMs: number | null;
  clipStartMs: number | null;
  clipTrackStartSec: number;
  clipRate: number;
  clipPath: string | null;
  loadSpanStartMs: number | null;
  loadSpanPath: string | null;
};

function makeDeckState(): DeckState {
  return {
    path: null,
    trackPosSec: 0,
    rate: 1,
    loopStartSec: null,
    loopEndSec: null,
    loopActive: false,
    loopEngagedMs: null,
    clipStartMs: null,
    clipTrackStartSec: 0,
    clipRate: 1,
    clipPath: null,
    loadSpanStartMs: null,
    loadSpanPath: null
  };
}

function startClip(deck: DeckState, ms: number) {
  deck.clipStartMs = ms;
  deck.clipTrackStartSec = deck.trackPosSec;
  deck.clipRate = deck.rate;
  deck.clipPath = deck.path;
}

function engageLoop(deck: DeckState, ms: number) {
  deck.loopActive = true;
  deck.loopEngagedMs = ms;
  deck.clipPath = deck.path;
  deck.clipRate = deck.rate;
  deck.clipStartMs = ms;
  if (deck.loopStartSec !== null) deck.trackPosSec = deck.loopStartSec;
}

function loopExitTrackPos(deck: DeckState, endMs: number): number {
  if (
    deck.loopEngagedMs === null ||
    deck.loopStartSec === null ||
    deck.loopEndSec === null ||
    deck.clipRate <= 0
  ) {
    return deck.trackPosSec;
  }
  const loopDurMs = ((deck.loopEndSec - deck.loopStartSec) / deck.clipRate) * 1000;
  if (loopDurMs <= 0) return deck.trackPosSec;
  const elapsedMs = endMs - deck.loopEngagedMs;
  const partialMs = elapsedMs % loopDurMs;
  return deck.loopStartSec + (partialMs / 1000) * deck.clipRate;
}

function finalizeClip(
  deck: DeckState,
  deckId: string,
  endMs: number,
  out: Clip[],
  nameForPath: (path: string) => string
) {
  if (
    deck.loopActive &&
    deck.loopEngagedMs !== null &&
    deck.loopStartSec !== null &&
    deck.loopEndSec !== null &&
    deck.clipPath !== null
  ) {
    const loopDurSec = deck.loopEndSec - deck.loopStartSec;
    if (loopDurSec > 0 && deck.clipRate > 0) {
      const loopDurMs = (loopDurSec / deck.clipRate) * 1000;
      const loopPath = deck.clipPath;
      const loopStartSec = deck.loopStartSec;
      const loopRate = deck.clipRate;
      let iterStart = deck.loopEngagedMs;
      while (iterStart < endMs) {
        const iterEnd = Math.min(iterStart + loopDurMs, endMs);
        out.push({
          deck: deckId,
          sessionStartMs: iterStart,
          sessionEndMs: iterEnd,
          trackPath: loopPath,
          trackName: nameForPath(loopPath),
          trackStartSec: loopStartSec,
          playbackRate: loopRate
        });
        iterStart += loopDurMs;
      }
    }
    deck.loopActive = false;
    deck.loopEngagedMs = null;
    deck.clipStartMs = null;
  } else if (deck.clipStartMs !== null && deck.clipPath !== null && endMs > deck.clipStartMs) {
    out.push({
      deck: deckId,
      sessionStartMs: deck.clipStartMs,
      sessionEndMs: endMs,
      trackPath: deck.clipPath,
      trackName: nameForPath(deck.clipPath),
      trackStartSec: deck.clipTrackStartSec,
      playbackRate: deck.clipRate
    });
    deck.clipStartMs = null;
  }
}

function finalizeLoadedSpan(
  deck: DeckState,
  deckId: string,
  endMs: number,
  out: LoadedSpan[],
  nameForPath: (path: string) => string
) {
  if (deck.loadSpanStartMs === null || deck.loadSpanPath === null) return;
  out.push({
    deck: deckId,
    trackPath: deck.loadSpanPath,
    trackName: nameForPath(deck.loadSpanPath),
    startMs: deck.loadSpanStartMs,
    endMs
  });
  deck.loadSpanStartMs = null;
  deck.loadSpanPath = null;
}

function buildClips(
  events: SessionEvent[],
  nameForPath: (path: string) => string
): { clips: Clip[]; loadedSpans: LoadedSpan[] } {
  const deckStates: Record<string, DeckState> = {};
  const clips: Clip[] = [];
  const loadedSpans: LoadedSpan[] = [];
  const getOrCreate = (id: string) => (deckStates[id] ??= makeDeckState());

  for (const ev of events) {
    const deckId = ev.deck;
    if (!deckId) continue;
    const deck = getOrCreate(deckId);

    switch (ev.type) {
      case 'deck_snapshot':
        deck.path = ev.path ?? null;
        deck.rate = ev.playback_rate ?? 1;
        deck.trackPosSec = ev.position_sec ?? 0;
        deck.loopStartSec =
          ev.loop_active && ev.loop_start_sec !== undefined ? ev.loop_start_sec : null;
        deck.loopEndSec = ev.loop_active && ev.loop_end_sec !== undefined ? ev.loop_end_sec : null;
        if (deck.path !== null && deck.loadSpanStartMs === null) {
          deck.loadSpanStartMs = 0;
          deck.loadSpanPath = deck.path;
        }
        if (ev.is_playing) {
          if (ev.loop_active && deck.loopStartSec !== null && deck.loopEndSec !== null) {
            engageLoop(deck, ev.elapsed_ms);
          } else {
            startClip(deck, ev.elapsed_ms);
          }
        }
        break;

      case 'load_track':
        finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath);
        finalizeLoadedSpan(deck, deckId, ev.elapsed_ms, loadedSpans, nameForPath);
        deck.path = ev.path ?? null;
        deck.loadSpanStartMs = ev.elapsed_ms;
        deck.loadSpanPath = deck.path;
        deck.trackPosSec = 0;
        deck.loopStartSec = null;
        deck.loopEndSec = null;
        deck.loopActive = false;
        deck.loopEngagedMs = null;
        break;

      case 'eject_track':
        finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath);
        finalizeLoadedSpan(deck, deckId, ev.elapsed_ms, loadedSpans, nameForPath);
        deck.path = null;
        deck.trackPosSec = 0;
        deck.loopActive = false;
        deck.loopEngagedMs = null;
        break;

      case 'play':
        if (deck.clipStartMs === null && !deck.loopActive) startClip(deck, ev.elapsed_ms);
        break;

      case 'cue_preview_start':
        if (deck.clipStartMs === null && !deck.loopActive) startClip(deck, ev.elapsed_ms);
        break;

      case 'stop':
      case 'stopped_at_cue':
      case 'stop_at_cue':
        finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath);
        if (ev.cue_point_sec !== undefined) deck.trackPosSec = ev.cue_point_sec;
        break;

      case 'cue_preview_end':
        finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath);
        if (ev.cue_point_sec !== undefined) deck.trackPosSec = ev.cue_point_sec;
        break;

      case 'seek':
        if (ev.sec !== undefined) {
          if (deck.clipStartMs !== null && !deck.loopActive) {
            finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath);
            deck.trackPosSec = ev.sec;
            startClip(deck, ev.elapsed_ms);
          } else {
            deck.trackPosSec = ev.sec;
          }
        }
        break;

      case 'set_playback_rate':
        if (ev.rate !== undefined) deck.rate = ev.rate;
        break;

      case 'loop_out':
        deck.loopStartSec = ev.start_sec ?? null;
        deck.loopEndSec = ev.end_sec ?? null;
        if (deck.loopStartSec !== null && deck.loopEndSec !== null) {
          finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath);
          engageLoop(deck, ev.elapsed_ms);
        }
        break;

      case 'loop_in':
        if (deck.loopActive) {
          deck.trackPosSec = loopExitTrackPos(deck, ev.elapsed_ms);
          finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath);
          startClip(deck, ev.elapsed_ms);
        }
        deck.loopStartSec = null;
        deck.loopEndSec = null;
        break;

      case 'exit_loop':
        if (deck.loopActive) {
          deck.trackPosSec = loopExitTrackPos(deck, ev.elapsed_ms);
          finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath);
          startClip(deck, ev.elapsed_ms);
        }
        break;

      case 'reloop':
        if (!deck.loopActive && deck.loopStartSec !== null && deck.loopEndSec !== null) {
          finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath);
          engageLoop(deck, ev.elapsed_ms);
        }
        break;

      case 'set_loop_active':
        if (
          ev.active &&
          !deck.loopActive &&
          deck.loopStartSec !== null &&
          deck.loopEndSec !== null
        ) {
          finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath);
          engageLoop(deck, ev.elapsed_ms);
        } else if (!ev.active && deck.loopActive) {
          deck.trackPosSec = loopExitTrackPos(deck, ev.elapsed_ms);
          finalizeClip(deck, deckId, ev.elapsed_ms, clips, nameForPath);
          startClip(deck, ev.elapsed_ms);
        }
        break;

      case 'set_loop_region':
        deck.loopStartSec = ev.start_sec ?? null;
        deck.loopEndSec = ev.end_sec ?? null;
        break;
    }
  }

  const lastMs = events[events.length - 1]?.elapsed_ms ?? 0;
  for (const [deckId, deck] of Object.entries(deckStates)) {
    finalizeClip(deck, deckId, lastMs, clips, nameForPath);
    finalizeLoadedSpan(deck, deckId, lastMs, loadedSpans, nameForPath);
  }

  return { clips, loadedSpans };
}

export type ParsedSession = {
  events: SessionEvent[];
  durationMs: number;
};

function defaultNameForPath(path: string): string {
  return (
    path
      .split('/')
      .pop()
      ?.replace(/\.[^.]+$/, '') ?? path
  );
}

export function useSessionTimeline(
  session: Ref<ParsedSession | null>,
  nameForPath: (path: string) => string = defaultNameForPath
) {
  const built = computed(() => {
    if (!session.value) return { clips: [] as Clip[], loadedSpans: [] as LoadedSpan[] };
    return buildClips(session.value.events, nameForPath);
  });

  const clips = computed(() => built.value.clips);
  const loadedSpans = computed(() => built.value.loadedSpans);

  return { clips, loadedSpans };
}
