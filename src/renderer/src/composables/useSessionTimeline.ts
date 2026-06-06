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
  trackStartSec: number;
  playbackRate: number;
  loopStartSec: number | null;
  loopEndSec: number | null;
  loopEngagedAtMs: number | null;
};

type DS = {
  path: string | null;
  trackPosSec: number;
  rate: number;
  loopStartSec: number | null;
  loopEndSec: number | null;
  loopEngagedAtMs: number | null;
  clipStartMs: number | null;
  clipTrackStartSec: number;
  clipRate: number;
  clipPath: string | null;
};

function makeDS(): DS {
  return {
    path: null,
    trackPosSec: 0,
    rate: 1,
    loopStartSec: null,
    loopEndSec: null,
    loopEngagedAtMs: null,
    clipStartMs: null,
    clipTrackStartSec: 0,
    clipRate: 1,
    clipPath: null
  };
}

function startClip(d: DS, ms: number) {
  d.clipStartMs = ms;
  d.clipTrackStartSec = d.trackPosSec;
  d.clipRate = d.rate;
  d.clipPath = d.path;
}

function finalizeClip(d: DS, deckId: string, endMs: number, out: Clip[]) {
  if (d.clipStartMs === null || d.clipPath === null || endMs <= d.clipStartMs) return;
  out.push({
    deck: deckId,
    sessionStartMs: d.clipStartMs,
    sessionEndMs: endMs,
    trackPath: d.clipPath,
    trackStartSec: d.clipTrackStartSec,
    playbackRate: d.clipRate,
    loopStartSec: d.loopStartSec,
    loopEndSec: d.loopEndSec,
    loopEngagedAtMs: d.loopEngagedAtMs
  });
  d.clipStartMs = null;
}

function buildClips(events: SessionEvent[]): Clip[] {
  const states: Record<string, DS> = {};
  const clips: Clip[] = [];
  const get = (id: string) => (states[id] ??= makeDS());

  for (const ev of events) {
    const id = ev.deck;
    if (!id) continue;
    const d = get(id);

    switch (ev.type) {
      case 'deck_snapshot':
        d.path = ev.path ?? null;
        d.rate = ev.playback_rate ?? 1;
        d.trackPosSec = ev.position_sec ?? 0;
        d.loopStartSec =
          ev.loop_active && ev.loop_start_sec !== undefined ? ev.loop_start_sec : null;
        d.loopEndSec = ev.loop_active && ev.loop_end_sec !== undefined ? ev.loop_end_sec : null;
        d.loopEngagedAtMs = ev.loop_active ? ev.elapsed_ms : null;
        if (ev.is_playing) startClip(d, ev.elapsed_ms);
        break;

      case 'load_track':
        finalizeClip(d, id, ev.elapsed_ms, clips);
        d.path = ev.path ?? null;
        d.trackPosSec = 0;
        d.loopStartSec = null;
        d.loopEndSec = null;
        d.loopEngagedAtMs = null;
        break;

      case 'eject_track':
        finalizeClip(d, id, ev.elapsed_ms, clips);
        d.path = null;
        d.trackPosSec = 0;
        break;

      case 'play':
        if (d.clipStartMs === null) startClip(d, ev.elapsed_ms);
        break;

      case 'stop':
      case 'stopped_at_cue':
      case 'stop_at_cue':
        finalizeClip(d, id, ev.elapsed_ms, clips);
        if (ev.cue_point_sec !== undefined) d.trackPosSec = ev.cue_point_sec;
        break;

      case 'seek':
        if (ev.sec !== undefined) {
          if (d.clipStartMs !== null) {
            finalizeClip(d, id, ev.elapsed_ms, clips);
            d.trackPosSec = ev.sec;
            startClip(d, ev.elapsed_ms);
          } else {
            d.trackPosSec = ev.sec;
          }
        }
        break;

      case 'set_playback_rate':
        if (ev.rate !== undefined) d.rate = ev.rate;
        break;

      case 'loop_out':
        d.loopStartSec = ev.start_sec ?? null;
        d.loopEndSec = ev.end_sec ?? null;
        d.loopEngagedAtMs = ev.elapsed_ms;
        break;

      case 'loop_in':
        d.loopStartSec = null;
        d.loopEndSec = null;
        d.loopEngagedAtMs = null;
        break;

      case 'exit_loop':
        d.loopEngagedAtMs = null;
        break;

      case 'reloop':
        if (d.loopStartSec !== null) d.loopEngagedAtMs = ev.elapsed_ms;
        break;
    }
  }

  const lastMs = events[events.length - 1]?.elapsed_ms ?? 0;
  for (const [id, d] of Object.entries(states)) {
    finalizeClip(d, id, lastMs, clips);
  }

  return clips;
}

export type ParsedSession = {
  events: SessionEvent[];
  durationMs: number;
};

export function useSessionTimeline(session: Ref<ParsedSession | null>) {
  const clips = computed<Clip[]>(() => {
    if (!session.value) return [];
    return buildClips(session.value.events);
  });

  return { clips };
}
