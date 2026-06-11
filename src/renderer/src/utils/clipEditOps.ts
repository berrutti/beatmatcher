import type { SessionEvent } from '@renderer/stores/session';
import type { Clip } from '@renderer/composables/useSessionTimeline';

// A user-draggable unit on the timeline: one regular play segment, or one run
// of loop iterations (which always moves as a whole). Derived from buildClips
// output, so every field reflects what the listener actually heard.
export type TransportBlock = {
  deck: string;
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

function near(a: number, b: number): boolean {
  return Math.abs(a - b) <= EPS_MS;
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
    return [
      { elapsed_ms: atMs, type: 'play', deck: block.deck, sec: block.loop.startSec },
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
  index: number;
  prev: TransportBlock | null;
  next: TransportBlock | null;
  minStartMs: number;
  maxEndMs: number;
};

// Finds the block in its deck's sequence and the range its boundaries may
// occupy: neighbor blocks clamp, and load_track/eject_track events are hard
// barriers because crossing one would put playback on a different track.
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

  let minStartMs = Math.max(0, prev?.endMs ?? 0);
  let maxEndMs = next?.startMs ?? Infinity;
  for (const event of events) {
    if (event.deck !== block.deck) continue;
    if (event.type !== 'load_track' && event.type !== 'eject_track') continue;
    if (event.elapsed_ms <= block.startMs + EPS_MS) {
      minStartMs = Math.max(minStartMs, event.elapsed_ms);
    } else if (event.elapsed_ms >= block.endMs - EPS_MS) {
      maxEndMs = Math.min(maxEndMs, event.elapsed_ms);
    }
  }
  return { index, prev, next, minStartMs, maxEndMs };
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
  const hood = neighborhoodOf(events, clips, block);
  return hood ? { minStartMs: hood.minStartMs, maxEndMs: hood.maxEndMs } : null;
}

export function moveTransportBlock(
  events: SessionEvent[],
  clips: Clip[],
  block: TransportBlock,
  deltaMs: number
): { events: SessionEvent[]; appliedDeltaMs: number } {
  const hood = neighborhoodOf(events, clips, block);
  if (!hood) return { events, appliedDeltaMs: 0 };
  const { prev, next, minStartMs, maxEndMs } = hood;
  const t0 = block.startMs;
  const t1 = block.endMs;

  const applied = Math.max(minStartMs - t0, Math.min(maxEndMs - t1, deltaMs));
  if (Math.abs(applied) < 1) return { events, appliedDeltaMs: 0 };

  const prevGlued = prev !== null && near(prev.endMs, t0);
  const nextGlued = next !== null && near(next.startMs, t1);

  const kept: SessionEvent[] = [];
  for (const event of events) {
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
  if (prevGlued) inserted.push({ elapsed_ms: t0, type: 'stop', deck: block.deck });
  if (nextGlued && next) inserted.push(...startEventsFor(next, t1));
  inserted.push(...startEventsFor(block, t0 + applied));
  inserted.push(...endEventsFor(block, t1 + applied));

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
  const hood = neighborhoodOf(events, clips, block);
  if (!hood) return { events, appliedMs: edge === 'start' ? block.startMs : block.endMs };
  const { prev, next, minStartMs, maxEndMs } = hood;
  const t0 = block.startMs;
  const t1 = block.endMs;

  if (edge === 'start') {
    // Audio stays aligned in session time: the clip starts later (or earlier)
    // but plays the audio it would have been playing at that moment.
    const earliestByAudio = t0 - (block.trackStartSec / block.playbackRate) * 1000;
    const lower = Math.max(minStartMs, earliestByAudio);
    const applied = Math.max(lower, Math.min(t1 - MIN_BLOCK_MS, newMs));
    if (near(applied, t0)) return { events, appliedMs: t0 };
    const newSec = block.trackStartSec + ((applied - t0) / 1000) * block.playbackRate;

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
    if (prevGlued) inserted.push({ elapsed_ms: t0, type: 'stop', deck: block.deck });
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
