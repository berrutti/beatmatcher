import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('@tauri-apps/plugin-store', () => ({ load: vi.fn() }));

import { buildClips } from '@renderer/composables/useSessionTimeline';
import {
  blocksForDeck,
  moveTransportBlock,
  trimTransportBlock,
  MIN_BLOCK_MS
} from '../clipEditOps';
import type { SessionEvent } from '@renderer/stores/session';

const TRACK = '/music/a.mp3';
const nameForPath = (path: string) => path;

function ev(overrides: Partial<SessionEvent> & { elapsed_ms: number; type: string }): SessionEvent {
  return { deck: 'A', ...overrides } as SessionEvent;
}

function snapshot(overrides: Partial<SessionEvent> = {}): SessionEvent {
  return ev({
    elapsed_ms: 0,
    type: 'deck_snapshot',
    path: TRACK,
    is_playing: false,
    position_sec: 0,
    playback_rate: 1,
    ...overrides
  });
}

function clipsOf(events: SessionEvent[]) {
  return buildClips(events, nameForPath).clips;
}

function deckBlocks(events: SessionEvent[], deck = 'A') {
  return blocksForDeck(clipsOf(events), deck);
}

// Spans of [sessionStartMs, sessionEndMs, trackStartSec] for quick comparison.
function spans(events: SessionEvent[], deck = 'A') {
  return clipsOf(events)
    .filter((clip) => clip.deck === deck)
    .map((clip) => [clip.sessionStartMs, clip.sessionEndMs, clip.trackStartSec]);
}

describe('blocksForDeck', () => {
  it('groups loop iterations into a single block', () => {
    const events = [
      snapshot({ is_playing: true }),
      ev({ elapsed_ms: 4000, type: 'loop_out', start_sec: 8, end_sec: 10 }),
      ev({ elapsed_ms: 9000, type: 'exit_loop' }),
      ev({ elapsed_ms: 12_000, type: 'stop' })
    ];
    const blocks = deckBlocks(events);
    expect(blocks.length).toBe(3);
    expect(blocks[0]).toMatchObject({ startMs: 0, endMs: 4000, loop: null });
    expect(blocks[1]).toMatchObject({
      startMs: 4000,
      endMs: 9000,
      loop: { startSec: 8, endSec: 10 }
    });
    expect(blocks[2]).toMatchObject({ startMs: 9000, endMs: 12_000, loop: null });
  });
});

describe('moveTransportBlock', () => {
  const simpleEvents = [
    snapshot({ position_sec: 5 }),
    ev({ elapsed_ms: 1000, type: 'play' }),
    ev({ elapsed_ms: 1500, type: 'set_volume', gain: 0.5 }),
    ev({ elapsed_ms: 3000, type: 'stop' })
  ];

  it('moves a clip into silence, audio preserved', () => {
    const blocks = deckBlocks(simpleEvents);
    const result = moveTransportBlock(simpleEvents, clipsOf(simpleEvents), blocks[0], 500);
    expect(result.appliedDeltaMs).toBe(500);
    expect(spans(result.events)).toEqual([[1500, 3500, 5]]);
  });

  it('moves a clip left', () => {
    const blocks = deckBlocks(simpleEvents);
    const result = moveTransportBlock(simpleEvents, clipsOf(simpleEvents), blocks[0], -500);
    expect(result.appliedDeltaMs).toBe(-500);
    expect(spans(result.events)).toEqual([[500, 2500, 5]]);
  });

  it('leaves automation events at wall time', () => {
    const blocks = deckBlocks(simpleEvents);
    const result = moveTransportBlock(simpleEvents, clipsOf(simpleEvents), blocks[0], 500);
    const volume = result.events.find((event) => event.type === 'set_volume');
    expect(volume?.elapsed_ms).toBe(1500);
  });

  it('clamps at session start', () => {
    const blocks = deckBlocks(simpleEvents);
    const result = moveTransportBlock(simpleEvents, clipsOf(simpleEvents), blocks[0], -5000);
    expect(result.appliedDeltaMs).toBe(-1000);
    expect(spans(result.events)).toEqual([[0, 2000, 5]]);
  });

  it('clamps against neighbor blocks', () => {
    const events = [
      snapshot(),
      ev({ elapsed_ms: 1000, type: 'play' }),
      ev({ elapsed_ms: 3000, type: 'stop' }),
      ev({ elapsed_ms: 4000, type: 'play' }),
      ev({ elapsed_ms: 6000, type: 'stop' })
    ];
    const blocks = deckBlocks(events);
    const right = moveTransportBlock(events, clipsOf(events), blocks[0], 5000);
    expect(right.appliedDeltaMs).toBe(1000);
    const left = moveTransportBlock(events, clipsOf(events), blocks[1], -5000);
    expect(left.appliedDeltaMs).toBe(-1000);
  });

  it('returns the same array when fully clamped', () => {
    const events = [
      snapshot({ is_playing: true }),
      ev({ elapsed_ms: 2000, type: 'seek', sec: 30 }),
      ev({ elapsed_ms: 5000, type: 'stop' })
    ];
    // First block is glued on both sides (session start, adjacent next block).
    const blocks = deckBlocks(events);
    const result = moveTransportBlock(events, clipsOf(events), blocks[0], 1000);
    expect(result.appliedDeltaMs).toBe(0);
    expect(result.events).toBe(events);
  });

  it('moving the right half of a seek split keeps the left half intact', () => {
    const events = [
      snapshot({ is_playing: true }),
      ev({ elapsed_ms: 2000, type: 'seek', sec: 30 }),
      ev({ elapsed_ms: 5000, type: 'stop' })
    ];
    const blocks = deckBlocks(events);
    const result = moveTransportBlock(events, clipsOf(events), blocks[1], 1000);
    expect(result.appliedDeltaMs).toBe(1000);
    expect(spans(result.events)).toEqual([
      [0, 2000, 0],
      [3000, 6000, 30]
    ]);
  });

  it('normalizes a resume-play in the next block when the moved end event vanishes', () => {
    const events = [
      snapshot({ is_playing: true }),
      ev({ elapsed_ms: 2000, type: 'stopped_at_cue', cue_point_sec: 10 }),
      ev({ elapsed_ms: 4000, type: 'play' }),
      ev({ elapsed_ms: 6000, type: 'stop' })
    ];
    const blocks = deckBlocks(events);
    const result = moveTransportBlock(events, clipsOf(events), blocks[0], 500);
    expect(result.appliedDeltaMs).toBe(500);
    // The second clip must still play from 10s even though stopped_at_cue
    // (which set that position) was replaced by a synthesized stop.
    expect(spans(result.events)).toEqual([
      [500, 2500, 0],
      [4000, 6000, 10]
    ]);
  });

  it('moves a loop block as one unit', () => {
    const events = [
      snapshot({ position_sec: 8 }),
      ev({ elapsed_ms: 2000, type: 'play' }),
      ev({ elapsed_ms: 2000, type: 'loop_out', start_sec: 8, end_sec: 10 }),
      ev({ elapsed_ms: 7000, type: 'stop' })
    ];
    const blocks = deckBlocks(events);
    expect(blocks.length).toBe(1);
    const result = moveTransportBlock(events, clipsOf(events), blocks[0], 1000);
    expect(result.appliedDeltaMs).toBe(1000);
    expect(spans(result.events)).toEqual([
      [3000, 5000, 8],
      [5000, 7000, 8],
      [7000, 8000, 8]
    ]);
  });

  it('moving a loop block away from its continuation keeps the continuation audio', () => {
    const events = [
      snapshot({ position_sec: 8 }),
      ev({ elapsed_ms: 2000, type: 'play' }),
      ev({ elapsed_ms: 2000, type: 'loop_out', start_sec: 8, end_sec: 10 }),
      ev({ elapsed_ms: 6000, type: 'exit_loop' }),
      ev({ elapsed_ms: 9000, type: 'stop' })
    ];
    const blocks = deckBlocks(events);
    expect(blocks.length).toBe(2);
    // Loop block [2000..6000], continuation [6000..9000] from 8s (loop phase 0).
    const result = moveTransportBlock(events, clipsOf(events), blocks[0], -1000);
    expect(result.appliedDeltaMs).toBe(-1000);
    expect(spans(result.events)).toEqual([
      [1000, 3000, 8],
      [3000, 5000, 8],
      [6000, 9000, 8]
    ]);
  });

  it('does not move across a load_track barrier', () => {
    const events = [
      snapshot({ position_sec: 5 }),
      ev({ elapsed_ms: 1000, type: 'play' }),
      ev({ elapsed_ms: 3000, type: 'stop' }),
      ev({ elapsed_ms: 4000, type: 'load_track', path: '/music/b.mp3', beat_offset_sec: 0 })
    ];
    const blocks = deckBlocks(events);
    const result = moveTransportBlock(events, clipsOf(events), blocks[0], 5000);
    expect(result.appliedDeltaMs).toBe(1000);
  });

  it('neuters a playing deck_snapshot when its block moves', () => {
    const events = [snapshot({ is_playing: true }), ev({ elapsed_ms: 3000, type: 'stop' })];
    const blocks = deckBlocks(events);
    const result = moveTransportBlock(events, clipsOf(events), blocks[0], 2000);
    expect(result.appliedDeltaMs).toBe(2000);
    expect(spans(result.events)).toEqual([[2000, 5000, 0]]);
    const snap = result.events.find((event) => event.type === 'deck_snapshot');
    expect(snap?.is_playing).toBe(false);
  });

  it('ignores other decks entirely', () => {
    const events = [
      snapshot({ position_sec: 5 }),
      ev({ elapsed_ms: 1000, type: 'play' }),
      ev({ elapsed_ms: 3000, type: 'stop' }),
      snapshot({ deck: 'B', is_playing: true, position_sec: 0 }),
      ev({ elapsed_ms: 8000, type: 'stop', deck: 'B' })
    ];
    const blocks = deckBlocks(events);
    const result = moveTransportBlock(events, clipsOf(events), blocks[0], 500);
    expect(spans(result.events, 'B')).toEqual([[0, 8000, 0]]);
  });
});

describe('trimTransportBlock', () => {
  const events = [
    snapshot({ position_sec: 5 }),
    ev({ elapsed_ms: 1000, type: 'play' }),
    ev({ elapsed_ms: 5000, type: 'stop' })
  ];

  it('trim start keeps audio aligned in session time', () => {
    const blocks = deckBlocks(events);
    const result = trimTransportBlock(events, clipsOf(events), blocks[0], 'start', 2000);
    expect(result.appliedMs).toBe(2000);
    expect(spans(result.events)).toEqual([[2000, 5000, 6]]);
  });

  it('trim start can extend left while audio stays aligned', () => {
    const blocks = deckBlocks(events);
    const result = trimTransportBlock(events, clipsOf(events), blocks[0], 'start', 500);
    expect(result.appliedMs).toBe(500);
    expect(spans(result.events)).toEqual([[500, 5000, 4.5]]);
  });

  it('trim start clamps so the track position never goes negative', () => {
    const startEvents = [
      snapshot({ position_sec: 0.5 }),
      ev({ elapsed_ms: 1000, type: 'play' }),
      ev({ elapsed_ms: 5000, type: 'stop' })
    ];
    const blocks = deckBlocks(startEvents);
    const result = trimTransportBlock(startEvents, clipsOf(startEvents), blocks[0], 'start', 0);
    expect(result.appliedMs).toBe(500);
    expect(spans(result.events)).toEqual([[500, 5000, 0]]);
  });

  it('trim start respects the playback rate for the audio offset', () => {
    const rateEvents = [
      snapshot({ position_sec: 5, playback_rate: 1.5 }),
      ev({ elapsed_ms: 1000, type: 'play' }),
      ev({ elapsed_ms: 5000, type: 'stop' })
    ];
    const blocks = deckBlocks(rateEvents);
    const result = trimTransportBlock(rateEvents, clipsOf(rateEvents), blocks[0], 'start', 2000);
    expect(spans(result.events)).toEqual([[2000, 5000, 6.5]]);
  });

  it('trim end shrinks and extends within free space', () => {
    const blocks = deckBlocks(events);
    const shrunk = trimTransportBlock(events, clipsOf(events), blocks[0], 'end', 3000);
    expect(spans(shrunk.events)).toEqual([[1000, 3000, 5]]);
    const extended = trimTransportBlock(events, clipsOf(events), blocks[0], 'end', 8000);
    expect(spans(extended.events)).toEqual([[1000, 8000, 5]]);
  });

  it('trim end clamps against the next block and enforces a minimum length', () => {
    const twoBlocks = [
      snapshot({ position_sec: 5 }),
      ev({ elapsed_ms: 1000, type: 'play' }),
      ev({ elapsed_ms: 3000, type: 'stop' }),
      ev({ elapsed_ms: 4000, type: 'play' }),
      ev({ elapsed_ms: 6000, type: 'stop' })
    ];
    const blocks = deckBlocks(twoBlocks);
    const extended = trimTransportBlock(twoBlocks, clipsOf(twoBlocks), blocks[0], 'end', 9000);
    expect(extended.appliedMs).toBe(4000);
    const tiny = trimTransportBlock(twoBlocks, clipsOf(twoBlocks), blocks[0], 'end', 1001);
    expect(tiny.appliedMs).toBe(1000 + MIN_BLOCK_MS);
  });

  it('trim end with a glued next block reconstructs its start', () => {
    const glued = [
      snapshot({ is_playing: true }),
      ev({ elapsed_ms: 2000, type: 'seek', sec: 30 }),
      ev({ elapsed_ms: 5000, type: 'stop' })
    ];
    const blocks = deckBlocks(glued);
    const result = trimTransportBlock(glued, clipsOf(glued), blocks[0], 'end', 1500);
    expect(result.appliedMs).toBe(1500);
    expect(spans(result.events)).toEqual([
      [0, 1500, 0],
      [2000, 5000, 30]
    ]);
  });

  it('does not trim loop blocks', () => {
    const loopEvents = [
      snapshot({ position_sec: 8 }),
      ev({ elapsed_ms: 2000, type: 'play' }),
      ev({ elapsed_ms: 2000, type: 'loop_out', start_sec: 8, end_sec: 10 }),
      ev({ elapsed_ms: 7000, type: 'stop' })
    ];
    const blocks = deckBlocks(loopEvents);
    const result = trimTransportBlock(loopEvents, clipsOf(loopEvents), blocks[0], 'start', 3000);
    expect(result.events).toBe(loopEvents);
  });
});
