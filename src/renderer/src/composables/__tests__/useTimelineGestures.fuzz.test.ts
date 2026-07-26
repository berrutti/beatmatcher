import { describe, it, expect } from 'vitest';
import { useTimelineGestures, type GestureDeps } from '@renderer/composables/useTimelineGestures';
import { blockBounds, blocksForDeck, buildTimeline } from '@renderer/utils/sessionCore';
import { PITCH_RANGE_OPTIONS } from '@renderer/stores/settings';
import type { SceneItem, Hit, ViewContext } from '@renderer/utils/timelineEngine';
import type { Intent } from '@renderer/utils/timelineIntents';
import type { Clip, SessionEvent, TransportBlock } from '@renderer/utils/types';

// Deterministic so a failure is reproducible from the printed case index alone.
function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

const DURATION_MS = 120_000;
const TRACK_WIDTH = 800;

const viewContext = {
  view: { start: 0, duration: DURATION_MS },
  scrollY: 0,
  canvasW: 1000,
  canvasH: 1000,
  trackW: TRACK_WIDTH,
  msToX: (ms: number) => (ms / DURATION_MS) * TRACK_WIDTH,
  xToMs: (x: number) => (x / TRACK_WIDTH) * DURATION_MS,
  mixerId: 'classic-3band',
  laneOriginY: 0,
  scrollViewport: { top: 0, bottom: 1000 }
} as ViewContext;

// Non-overlapping play/stop pairs on one deck, which is what a recorder can
// produce: a deck plays one track at a time.
function randomSession(random: () => number): SessionEvent[] {
  const events: SessionEvent[] = [
    { elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/tracks/one.mp3' }
  ];
  let ms = Math.floor(random() * 2000);
  const clipCount = 1 + Math.floor(random() * 4);
  for (let index = 0; index < clipCount; index++) {
    const playMs = ms + Math.floor(random() * 4000);
    const stopMs = playMs + 500 + Math.floor(random() * 20_000);
    events.push({ elapsed_ms: playMs, type: 'play', deck: 'A' });
    events.push({ elapsed_ms: stopMs, type: 'stop', deck: 'A' });
    ms = stopMs;
  }
  return events;
}

function harness(clips: Clip[], events: SessionEvent[], hit: Hit) {
  const intents: Intent[] = [];
  const item: SceneItem = {
    bounds: () => ({ x: 0, y: 0, w: 1000, h: 1000 }),
    draw: () => {},
    hitTest: () => hit
  };
  const deps: GestureDeps = {
    camera: {
      currentView: () => ({ start: 0, duration: DURATION_MS }),
      panByPixels: () => {},
      panByMsDelta: () => {},
      zoomAt: () => {},
      maxScrollY: () => 0
    } as unknown as GestureDeps['camera'],
    emit: (intent) => intents.push(intent),
    getItems: () => [item],
    getRows: () => [],
    getVc: () => viewContext,
    getClips: () => clips,
    getEvents: () => events,
    getDeckLanes: () => ({}),
    laneHeight: () => 64,
    waveformHeight: () => 80,
    isEditMode: () => true,
    durationMs: () => DURATION_MS,
    nudgeDirectionAt: () => 1,
    nudgeSensitivity: () => 1,
    accentFor: () => '#ffffff',
    requestRender: () => {},
    setCursor: () => {}
  };
  return { gestures: useTimelineGestures(deps), intents };
}

const RECT = { left: 0, top: 0 } as DOMRect;
const mouseAt = (x: number, y: number) => ({ clientX: x, clientY: y }) as MouseEvent;

type Case = {
  index: number;
  block: TransportBlock;
  edge: 'start' | 'end' | null;
  intents: Intent[];
  bounds: NonNullable<ReturnType<typeof blockBounds>>;
};

// Pointer targets overshoot each block's legal range, so the clamps are under test.
function fuzzCases(count: number): Case[] {
  const cases: Case[] = [];
  for (let index = 0; index < count; index++) {
    const random = makeRandom(index + 1);
    const events = randomSession(random);
    const clips = buildTimeline(
      events,
      DURATION_MS,
      PITCH_RANGE_OPTIONS,
      (path) => path,
      () => null
    ).clips;
    const blocks = blocksForDeck(clips, 'A');
    if (blocks.length === 0) continue;
    const block = blocks[Math.floor(random() * blocks.length)];
    const bounds = blockBounds(events, clips, block);
    if (!bounds) continue;

    const edgeRoll = random();
    const edge = edgeRoll < 0.34 ? 'start' : edgeRoll < 0.68 ? 'end' : null;
    const part = edge ?? 'body';
    const { gestures, intents } = harness(clips, events, {
      target: 'clip',
      deck: 'A',
      part,
      data: { block, rowTop: 0 }
    });

    const grabMs = block.startMs + random() * (block.endMs - block.startMs);
    // Up to a whole session's worth of travel in either direction.
    const targetMs = (random() * 2 - 0.5) * DURATION_MS;
    gestures.onMouseDown(mouseAt(viewContext.msToX(grabMs), 10), RECT);
    gestures.onMouseMove(mouseAt(viewContext.msToX(targetMs), 10), RECT);
    gestures.onMouseUp();
    gestures.onClick(mouseAt(viewContext.msToX(targetMs), 10), RECT);

    cases.push({ index, block, edge, intents, bounds });
  }
  return cases;
}

const CASES = fuzzCases(3000);

describe('clip gesture fuzz', () => {
  it('commits a real edit in nearly every case', () => {
    const committed = CASES.filter((one) =>
      one.intents.some((intent) => intent.type === 'clip.move' || intent.type === 'clip.trim')
    );
    expect(CASES.length).toBeGreaterThan(2500);
    expect(committed.length).toBeGreaterThan(CASES.length * 0.9);
    expect(CASES.some((one) => one.edge === null)).toBe(true);
    expect(CASES.some((one) => one.edge === 'start')).toBe(true);
    expect(CASES.some((one) => one.edge === 'end')).toBe(true);
  });

  it('never moves a block outside the range blockBounds allows', () => {
    for (const one of CASES) {
      const move = one.intents.find((intent) => intent.type === 'clip.move');
      if (move?.type !== 'clip.move') continue;
      const start = one.block.startMs + move.deltaMs;
      const end = one.block.endMs + move.deltaMs;
      expect(start, `case ${one.index} start`).toBeGreaterThanOrEqual(one.bounds.minStartMs - 1e-6);
      expect(end, `case ${one.index} end`).toBeLessThanOrEqual(one.bounds.maxEndMs + 1e-6);
    }
  });

  it('never trims a block below the minimum length or past its neighbours', () => {
    for (const one of CASES) {
      const trim = one.intents.find((intent) => intent.type === 'clip.trim');
      if (trim?.type !== 'clip.trim') continue;
      const { minBlockMs, maxEndMs, startTrimMinMs } = one.bounds;
      if (trim.edge === 'start') {
        expect(trim.newMs, `case ${one.index}`).toBeGreaterThanOrEqual(startTrimMinMs - 1e-6);
        expect(one.block.endMs - trim.newMs, `case ${one.index}`).toBeGreaterThanOrEqual(
          minBlockMs - 1e-6
        );
      } else {
        expect(trim.newMs, `case ${one.index}`).toBeLessThanOrEqual(maxEndMs + 1e-6);
        expect(trim.newMs - one.block.startMs, `case ${one.index}`).toBeGreaterThanOrEqual(
          minBlockMs - 1e-6
        );
      }
    }
  });

  it('never both edits and seeks in one gesture', () => {
    for (const one of CASES) {
      const types = one.intents.map((intent) => intent.type);
      const edited = types.includes('clip.move') || types.includes('clip.trim');
      if (edited) expect(types, `case ${one.index}`).not.toContain('seek');
    }
  });
});
