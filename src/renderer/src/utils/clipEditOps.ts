import type { SessionEvent } from '@renderer/stores/session';
import type { Clip } from '@renderer/composables/useSessionTimeline';

// A user-draggable unit on the timeline: one regular play segment, or one run
// of loop iterations (which always moves as a whole). Derived from buildClips
// output, so every field reflects what the listener actually heard.
export type TransportBlock = {
  deck: string;
  blockId: number;
  startMs: number;
  endMs: number;
  trackPath: string;
  trackStartSec: number;
  playbackRate: number;
  loop: { startSec: number; endSec: number } | null;
};

export const MIN_BLOCK_MS = 100;

const EPS_MS = 1;

// Every event type that changes deck position or play state. Mixer and
// metadata events (set_volume, set_eq, set_playback_rate, ...) are
// deliberately absent: automation stays at wall time when clips move.
const TRANSPORT_TYPES = new Set([
  'deck_snapshot',
  'load_track',
  'eject_track',
  'play',
  'stop',
  'stopped_at_cue',
  'stop_at_cue',
  'cue_set_and_stop',
  'cue_preview_start',
  'cue_preview_end',
  'cue_move',
  'seek',
  'loop_out',
  'loop_in',
  'exit_loop',
  'reloop'
]);

function near(first: number, second: number): boolean {
  return Math.abs(first - second) <= EPS_MS;
}

export function blocksForDeck(clips: Clip[], deck: string): TransportBlock[] {
  const byId = new Map<number, Clip[]>();
  for (const clip of clips) {
    if (clip.deck !== deck) continue;
    const group = byId.get(clip.blockId);
    if (group) group.push(clip);
    else byId.set(clip.blockId, [clip]);
  }
  const blocks: TransportBlock[] = [];
  for (const group of byId.values()) {
    const first = group[0];
    blocks.push({
      deck,
      blockId: first.blockId,
      startMs: Math.min(...group.map((clip) => clip.sessionStartMs)),
      endMs: Math.max(...group.map((clip) => clip.sessionEndMs)),
      trackPath: first.trackPath,
      trackStartSec: first.trackStartSec,
      playbackRate: first.playbackRate,
      loop: first.loop
    });
  }
  blocks.sort((left, right) => left.startMs - right.startMs);
  return blocks;
}

// Events that start a block from silence at an explicit position. Used both
// for the moved block at its new location and to reconstruct a neighbor whose
// shared boundary event was removed.
function startEventsFor(block: TransportBlock, atMs: number): SessionEvent[] {
  if (block.loop) {
    // trackStartSec is the wrapped entry position, which may sit inside the
    // region (the engine wraps a late loop_out press past the end); playing
    // from the loop start instead would shift the whole block by that offset.
    return [
      { elapsed_ms: atMs, type: 'play', deck: block.deck, sec: block.trackStartSec },
      {
        elapsed_ms: atMs,
        type: 'loop_out',
        deck: block.deck,
        start_sec: block.loop.startSec,
        end_sec: block.loop.endSec
      }
    ];
  }
  return [{ elapsed_ms: atMs, type: 'play', deck: block.deck, sec: block.trackStartSec }];
}

function endEventsFor(block: TransportBlock, atMs: number): SessionEvent[] {
  if (block.loop) {
    return [
      { elapsed_ms: atMs, type: 'exit_loop', deck: block.deck },
      { elapsed_ms: atMs, type: 'stop', deck: block.deck }
    ];
  }
  return [{ elapsed_ms: atMs, type: 'stop', deck: block.deck }];
}

function stableSortByMs(events: SessionEvent[]): SessionEvent[] {
  return events.sort((left, right) => left.elapsed_ms - right.elapsed_ms);
}

type Neighborhood = {
  prev: TransportBlock | null;
  next: TransportBlock | null;
  minStartMs: number;
  maxEndMs: number;
  // The load_track that loaded this block's track when it was loaded mid-session
  // (null when a deck_snapshot loaded it at session start). It rides ahead of
  // the block on a move and its load-to-play gap collapses; it never clamps.
  ownLoadMs: number | null;
};

// Finds the block in its deck's sequence and the range its boundaries may
// occupy. A block can move left until it meets the previous clip's end and
// right until the next clip's start (two tracks are never audible at once).
// Loads are cosmetic after recording, so they do not bar a leftward move (the
// block's own load rides with it); a load to the right still bars a move that
// would overrun it and leave the wrong track loaded under the play. A pinned
// session-start deck_snapshot and an eject_track bar both directions.
function neighborhoodOf(
  events: SessionEvent[],
  clips: Clip[],
  block: TransportBlock
): Neighborhood | null {
  const blocks = blocksForDeck(clips, block.deck);
  const index = blocks.findIndex(
    (candidate) => near(candidate.startMs, block.startMs) && near(candidate.endMs, block.endMs)
  );
  if (index === -1) return null;
  const prev = index > 0 ? blocks[index - 1] : null;
  const next = index < blocks.length - 1 ? blocks[index + 1] : null;

  let ownLoadMs: number | null = null;
  for (const event of events) {
    if (event.deck !== block.deck || event.type !== 'load_track') continue;
    if (event.elapsed_ms <= block.startMs + EPS_MS) {
      ownLoadMs = ownLoadMs === null ? event.elapsed_ms : Math.max(ownLoadMs, event.elapsed_ms);
    }
  }

  let minStartMs = Math.max(0, prev?.endMs ?? 0);
  let maxEndMs = next?.startMs ?? Infinity;
  for (const event of events) {
    if (event.deck !== block.deck) continue;
    const isLoad = event.type === 'load_track';
    const isLeftBarrier =
      event.type === 'eject_track' ||
      (event.type === 'deck_snapshot' && event.path !== undefined && event.path !== null);
    if (!isLoad && !isLeftBarrier) continue;
    if (event.elapsed_ms <= block.startMs + EPS_MS) {
      if (isLeftBarrier) minStartMs = Math.max(minStartMs, event.elapsed_ms);
    } else if (event.elapsed_ms >= block.endMs - EPS_MS) {
      maxEndMs = Math.min(maxEndMs, event.elapsed_ms);
    }
  }
  return { prev, next, minStartMs, maxEndMs, ownLoadMs };
}

// Signed audio seconds the deck advances between two session times, following
// the piecewise-constant rate curve and nudges (events are sorted by ms). The
// engine plays under this curve, so trims must use it: a single fixed rate
// lands the audio early or late whenever the pitch moved inside the window,
// heard as duplicated or skipped audio at the seam.
function audioSecondsBetween(
  events: SessionEvent[],
  deck: string,
  fromMs: number,
  toMs: number,
  fallbackRate: number
): number {
  if (fromMs === toMs) return 0;
  const sign = toMs >= fromMs ? 1 : -1;
  const lower = Math.min(fromMs, toMs);
  const upper = Math.max(fromMs, toMs);

  let rate = fallbackRate;
  let nudge = 1;
  let total = 0;
  let cursor = lower;
  for (const event of events) {
    if (event.deck !== deck) continue;
    if (event.elapsed_ms >= upper) break;
    const isRate = event.type === 'set_playback_rate' && event.rate !== undefined;
    const isSnapshot = event.type === 'deck_snapshot' && event.playback_rate !== undefined;
    const isNudge = event.type === 'set_nudge' && event.percent !== undefined;
    if (!isRate && !isSnapshot && !isNudge) continue;
    if (event.elapsed_ms > lower) {
      total += ((event.elapsed_ms - cursor) / 1000) * rate * nudge;
      cursor = event.elapsed_ms;
    }
    if (isRate && event.rate !== undefined) rate = event.rate;
    if (isSnapshot && event.playback_rate !== undefined) rate = event.playback_rate;
    if (isNudge && event.percent !== undefined) nudge = 1 + event.percent / 100;
  }
  total += ((upper - cursor) / 1000) * rate * nudge;
  return sign * total;
}

// A resume-play (no sec) takes its position from whatever the previous block's
// end event left behind. When boundary events are rewritten, that implicit
// dependency must become explicit; trackStartSec from buildClips is the
// rendered truth of where the clip actually started.
function normalizeResumePlay(event: SessionEvent, next: TransportBlock | null): SessionEvent {
  if (
    next &&
    event.type === 'play' &&
    event.sec === undefined &&
    near(event.elapsed_ms, next.startMs)
  ) {
    return { ...event, sec: next.trackStartSec };
  }
  return event;
}

// Handles a transport event sitting exactly on the block's start boundary.
// Returns the replacement event, or null when the event is consumed (the
// synthesized start takes over its role).
function rewriteStartBoundary(event: SessionEvent): SessionEvent | null {
  if (event.type === 'deck_snapshot') {
    // The snapshot also loads the track and seeds rate/cue state, so it must
    // survive; only its transport effects move with the block.
    return { ...event, is_playing: false, loop_active: false };
  }
  return null;
}

// The range the block's boundaries may occupy, for live drag previews that
// want to show the same clamping the commit will apply.
export function blockBounds(
  events: SessionEvent[],
  clips: Clip[],
  block: TransportBlock
): { minStartMs: number; maxEndMs: number } | null {
  const neighborhood = neighborhoodOf(events, clips, block);
  return neighborhood
    ? { minStartMs: neighborhood.minStartMs, maxEndMs: neighborhood.maxEndMs }
    : null;
}

// Loads carry no audio after recording, so an UNPLAYED track loaded inside the
// span the moved block now occupies is destroyed: its load_track and the setup
// config (rate, beat grid) it emitted, up to the next load, are dropped. A load
// followed by a play (a real played block, e.g. the next block when gluing
// right) is left untouched.
function orphanedLoadEvents(
  events: SessionEvent[],
  deck: string,
  ownLoadMs: number | null,
  spanStartMs: number,
  spanEndMs: number
): Set<SessionEvent> {
  const discarded = new Set<SessionEvent>();
  const loadMs = events
    .filter((event) => event.deck === deck && event.type === 'load_track')
    .map((event) => event.elapsed_ms)
    .sort((left, right) => left - right);
  const playMs = events
    .filter((event) => event.deck === deck && event.type === 'play')
    .map((event) => event.elapsed_ms);
  for (const load of events) {
    if (load.deck !== deck || load.type !== 'load_track') continue;
    if (ownLoadMs !== null && near(load.elapsed_ms, ownLoadMs)) continue;
    if (load.elapsed_ms < spanStartMs - EPS_MS || load.elapsed_ms > spanEndMs + EPS_MS) continue;
    const nextLoadMs = loadMs.find((ms) => ms > load.elapsed_ms + EPS_MS) ?? Infinity;
    const wasPlayed = playMs.some(
      (ms) => ms > load.elapsed_ms + EPS_MS && ms < nextLoadMs - EPS_MS
    );
    if (wasPlayed) continue;
    discarded.add(load);
    for (const event of events) {
      if (event.deck !== deck) continue;
      if (event.type !== 'set_playback_rate' && event.type !== 'set_beat_grid') continue;
      if (event.elapsed_ms >= load.elapsed_ms - EPS_MS && event.elapsed_ms < nextLoadMs - EPS_MS) {
        discarded.add(event);
      }
    }
  }
  return discarded;
}

export function moveTransportBlock(
  events: SessionEvent[],
  clips: Clip[],
  block: TransportBlock,
  deltaMs: number
): { events: SessionEvent[]; appliedDeltaMs: number } {
  const neighborhood = neighborhoodOf(events, clips, block);
  if (!neighborhood) return { events, appliedDeltaMs: 0 };
  const { prev, next, minStartMs, maxEndMs, ownLoadMs } = neighborhood;
  const t0 = block.startMs;
  const t1 = block.endMs;

  // Loads are cosmetic, so the only hard left limit is the previous clip's end
  // (two tracks are never audible at once). The block's own load rides just
  // ahead of the play, the load-to-play gap collapsing to zero on the way.
  const applied = Math.max(minStartMs - t0, Math.min(maxEndMs - t1, deltaMs));
  if (Math.abs(applied) < 1) return { events, appliedDeltaMs: 0 };

  const newStart = t0 + applied;
  const newEnd = t1 + applied;

  const newLoadMs = ownLoadMs === null ? null : Math.min(ownLoadMs, newStart);
  const loadShift = newLoadMs === null || ownLoadMs === null ? 0 : newLoadMs - ownLoadMs;
  // The block's own load_track and the config it sets at load (rate, beat grid)
  // ride with the block, folded just before the new play position so the track
  // stays loaded-then-configured before it plays. Automation (volume, eq,
  // filter) stays at wall time, as on any move.
  const inLoadWindow = (event: SessionEvent): boolean => {
    if (ownLoadMs === null || event.deck !== block.deck) return false;
    if (event.type === 'load_track') return near(event.elapsed_ms, ownLoadMs);
    if (event.type === 'set_playback_rate' || event.type === 'set_beat_grid') {
      return event.elapsed_ms >= ownLoadMs - EPS_MS && event.elapsed_ms < t0 - EPS_MS;
    }
    return false;
  };

  const discarded = orphanedLoadEvents(events, block.deck, ownLoadMs, newStart, newEnd);

  const prevGlued = prev !== null && near(prev.endMs, t0);
  const nextGlued = next !== null && near(next.startMs, t1);

  const kept: SessionEvent[] = [];
  for (const event of events) {
    if (discarded.has(event)) continue;
    if (inLoadWindow(event)) {
      kept.push({ ...event, elapsed_ms: Math.min(event.elapsed_ms + loadShift, newStart) });
      continue;
    }
    if (event.deck !== block.deck || !TRANSPORT_TYPES.has(event.type)) {
      kept.push(event);
      continue;
    }
    const ms = event.elapsed_ms;
    if (ms < t0 - EPS_MS || ms > t1 + EPS_MS) {
      kept.push(normalizeResumePlay(event, next));
      continue;
    }
    if (event.type === 'load_track' || event.type === 'eject_track') {
      kept.push(event);
      continue;
    }
    if (near(ms, t0)) {
      const replacement = rewriteStartBoundary(event);
      if (replacement) kept.push(replacement);
      continue;
    }
    if (near(ms, t1)) continue;
    kept.push({ ...event, elapsed_ms: ms + applied });
  }

  const inserted: SessionEvent[] = [];
  // endEventsFor, not a bare stop: a glued loop block must be disarmed with
  // exit_loop, because stop pauses the deck but leaves the loop armed and the
  // relocated clip would wrap at the stale loop boundary.
  if (prevGlued && prev) inserted.push(...endEventsFor(prev, t0));
  if (nextGlued && next) inserted.push(...startEventsFor(next, t1));
  inserted.push(...startEventsFor(block, newStart));
  inserted.push(...endEventsFor(block, newEnd));

  return { events: stableSortByMs([...kept, ...inserted]), appliedDeltaMs: applied };
}

export function trimTransportBlock(
  events: SessionEvent[],
  clips: Clip[],
  block: TransportBlock,
  edge: 'start' | 'end',
  newMs: number
): { events: SessionEvent[]; appliedMs: number } {
  // Loop blocks move as one unit; trimming them is not supported (a trimmed
  // engage point would shift the iteration phase, changing the audio).
  if (block.loop) {
    return { events, appliedMs: edge === 'start' ? block.startMs : block.endMs };
  }
  const neighborhood = neighborhoodOf(events, clips, block);
  if (!neighborhood) return { events, appliedMs: edge === 'start' ? block.startMs : block.endMs };
  const { prev, next, minStartMs, maxEndMs } = neighborhood;
  const t0 = block.startMs;
  const t1 = block.endMs;

  if (edge === 'start') {
    // Audio stays aligned in session time: the clip starts later (or earlier)
    // but plays the audio it would have been playing at that moment. The
    // earliest-by-audio bound is approximate (start rate only); the final
    // position floors at 0 below.
    const earliestByAudio = t0 - (block.trackStartSec / block.playbackRate) * 1000;
    const lower = Math.max(minStartMs, earliestByAudio);
    const applied = Math.max(lower, Math.min(t1 - MIN_BLOCK_MS, newMs));
    if (near(applied, t0)) return { events, appliedMs: t0 };
    const newSec = Math.max(
      0,
      block.trackStartSec +
        audioSecondsBetween(events, block.deck, t0, applied, block.playbackRate)
    );

    const prevGlued = prev !== null && near(prev.endMs, t0);
    const kept: SessionEvent[] = [];
    for (const event of events) {
      if (event.deck !== block.deck || !TRANSPORT_TYPES.has(event.type)) {
        kept.push(event);
        continue;
      }
      if (near(event.elapsed_ms, t0) && event.type !== 'load_track') {
        const replacement = rewriteStartBoundary(event);
        if (replacement) kept.push(replacement);
        continue;
      }
      kept.push(event);
    }
    const inserted: SessionEvent[] = [
      { elapsed_ms: applied, type: 'play', deck: block.deck, sec: newSec }
    ];
    if (prevGlued && prev) inserted.push(...endEventsFor(prev, t0));
    return { events: stableSortByMs([...kept, ...inserted]), appliedMs: applied };
  }

  const applied = Math.max(t0 + MIN_BLOCK_MS, Math.min(maxEndMs, newMs));
  if (near(applied, t1)) return { events, appliedMs: t1 };
  const nextGlued = next !== null && near(next.startMs, t1);

  const kept: SessionEvent[] = [];
  for (const event of events) {
    if (event.deck !== block.deck || !TRANSPORT_TYPES.has(event.type)) {
      kept.push(event);
      continue;
    }
    if (near(event.elapsed_ms, t1) && event.type !== 'load_track' && event.type !== 'eject_track') {
      continue;
    }
    kept.push(normalizeResumePlay(event, next));
  }
  const inserted: SessionEvent[] = [{ elapsed_ms: applied, type: 'stop', deck: block.deck }];
  if (nextGlued && next) inserted.push(...startEventsFor(next, t1));
  return { events: stableSortByMs([...kept, ...inserted]), appliedMs: applied };
}
