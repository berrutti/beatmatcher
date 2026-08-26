import { describe, it, expect } from 'vitest';

import {
  computeRowLayout,
  ghostSpan,
  clipGestureDeltaSec,
  selectionSpansFor,
  bpmRegionSpanAt,
  marqueeTargets,
  mergeSelectionRanges
} from '../timelineLayout';
import { ROW_H } from '../timelineDraw';
import type { Clip, TransportBlock } from '@renderer/utils/types';

function clip(overrides: Partial<Clip>): Clip {
  return {
    deck: 'A',
    sessionStartMs: 0,
    sessionEndMs: 1000,
    trackPath: '/t/a.mp3',
    trackName: 'a',
    trackStartSec: 0,
    playbackRate: 1,
    blockId: 0,
    loop: null,
    waveSegments: [],
    bpm: null,
    beatOffsetSec: null,
    ...overrides
  };
}

describe('computeRowLayout', () => {
  it('stacks rows and sublanes with cumulative tops', () => {
    const rows = computeRowLayout(
      [
        {
          deckId: 'A',
          laneHeights: [
            { key: 'gain', height: 16 },
            { key: 'filter', height: 64 }
          ]
        },
        { deckId: 'B', laneHeights: [] }
      ],
      16
    );
    expect(rows[0]).toMatchObject({ deckId: 'A', top: 16, height: ROW_H + 16 + 64 });
    expect(rows[0].lanes).toEqual([
      { key: 'gain', top: 16 + ROW_H, height: 16 },
      { key: 'filter', top: 16 + ROW_H + 16, height: 64 }
    ]);
    expect(rows[1]).toMatchObject({
      deckId: 'B',
      top: 16 + ROW_H + 16 + 64,
      height: ROW_H,
      lanes: []
    });
  });

  it('uses a custom waveform height for the strip and the lane offset', () => {
    const rows = computeRowLayout(
      [{ deckId: 'A', waveformHeight: 120, laneHeights: [{ key: 'filter', height: 64 }] }],
      0
    );
    expect(rows[0].waveformHeight).toBe(120);
    expect(rows[0].lanes[0].top).toBe(120); // lane sits directly below the waveform
    expect(rows[0].height).toBe(120 + 64);
  });

  it('sizes each deck waveform on its own, so one resize moves one deck', () => {
    const rows = computeRowLayout(
      [
        { deckId: 'A', waveformHeight: 120, laneHeights: [] },
        { deckId: 'B', waveformHeight: 40, laneHeights: [] }
      ],
      0
    );
    expect(rows[0].waveformHeight).toBe(120);
    expect(rows[1].waveformHeight).toBe(40);
    expect(rows[1].top).toBe(120);
  });
});

describe('ghostSpan', () => {
  const span = { sessionStartMs: 1000, sessionEndMs: 3000 };

  it('shifts both edges for a move', () => {
    expect(ghostSpan(span, { kind: 'move', deltaMs: 500, targetMs: 0 })).toEqual({
      startMs: 1500,
      endMs: 3500
    });
  });

  it('replaces the start edge for a start trim', () => {
    expect(ghostSpan(span, { kind: 'trim-start', deltaMs: 0, targetMs: 1800 })).toEqual({
      startMs: 1800,
      endMs: 3000
    });
  });

  it('replaces the end edge for an end trim', () => {
    expect(ghostSpan(span, { kind: 'trim-end', deltaMs: 0, targetMs: 2500 })).toEqual({
      startMs: 1000,
      endMs: 2500
    });
  });
});

describe('clipGestureDeltaSec', () => {
  it('uses deltaMs for moves and edge distance for trims', () => {
    expect(clipGestureDeltaSec('move', 500, 0, 1000, 3000)).toBeCloseTo(0.5, 6);
    expect(clipGestureDeltaSec('trim-start', 0, 1800, 1000, 3000)).toBeCloseTo(0.8, 6);
    expect(clipGestureDeltaSec('trim-end', 0, 2500, 1000, 3000)).toBeCloseTo(-0.5, 6);
  });
});

describe('selectionSpansFor', () => {
  it('returns only the spans on the requested deck', () => {
    expect(
      selectionSpansFor(
        [
          { deck: 'A', startMs: 1000, endMs: 3000 },
          { deck: 'B', startMs: 0, endMs: 9000 },
          { deck: 'A', startMs: 5000, endMs: 6000 }
        ],
        'A'
      )
    ).toEqual([
      { startMs: 1000, endMs: 3000 },
      { startMs: 5000, endMs: 6000 }
    ]);
  });
});

describe('bpmRegionSpanAt', () => {
  // 128 bpm grid. Segments at rate 1.0 (128.0), 1.0002 (128.0 displayed),
  // then 1.05 (134.4): the first two read as one region at 0.1 precision.
  const seg = (wallStartMs: number, wallEndMs: number, rate: number) => ({
    wallStartMs,
    wallEndMs,
    trackStartSec: 0,
    trackEndSec: ((wallEndMs - wallStartMs) / 1000) * rate
  });
  const regionClip = clip({
    sessionStartMs: 0,
    sessionEndMs: 30000,
    bpm: 128,
    waveSegments: [seg(0, 10000, 1.0), seg(10000, 20000, 1.0002), seg(20000, 30000, 1.05)]
  });

  it('merges adjacent segments whose displayed bpm matches at 0.1 precision', () => {
    expect(bpmRegionSpanAt(regionClip, 5000)).toEqual({ startMs: 0, endMs: 20000 });
    expect(bpmRegionSpanAt(regionClip, 15000)).toEqual({ startMs: 0, endMs: 20000 });
  });

  it('picks the differently pitched region on its own', () => {
    expect(bpmRegionSpanAt(regionClip, 25000)).toEqual({ startMs: 20000, endMs: 30000 });
  });

  it('falls back to the whole clip without a grid', () => {
    const noGrid = clip({ sessionStartMs: 0, sessionEndMs: 30000, bpm: null });
    expect(bpmRegionSpanAt(noGrid, 5000)).toEqual({ startMs: 0, endMs: 30000 });
  });
});

describe('marqueeTargets', () => {
  // 1px per 100ms in both directions.
  const xToMs = (x: number) => x * 100;

  function block(overrides: Partial<TransportBlock>): TransportBlock {
    return {
      deck: 'A',
      blockId: 0,
      startMs: 0,
      endMs: 1000,
      trackPath: '/t/a.mp3',
      trackStartSec: 0,
      playbackRate: 1,
      loop: null,
      ...overrides
    };
  }

  const rows = [
    { deckId: 'A', top: 0, waveformHeight: 80 },
    { deckId: 'B', top: 100, waveformHeight: 80 }
  ];
  const byDeck: Record<string, TransportBlock[]> = {
    A: [
      block({ blockId: 0, startMs: 0, endMs: 1000 }),
      block({ blockId: 1, startMs: 5000, endMs: 6000 }),
      block({ blockId: 2, startMs: 6000, endMs: 10000 })
    ],
    B: [block({ deck: 'B', blockId: 3, startMs: 5500, endMs: 9000 })]
  };
  const blocksFor = (deck: string) => byDeck[deck] ?? [];

  it('clips the rect time range to each touched block on crossed decks', () => {
    expect(marqueeTargets(rows, blocksFor, { x0: 45, x1: 65, y0: 10, y1: 60 }, xToMs)).toEqual([
      { deck: 'A', startMs: 5000, endMs: 6000 },
      { deck: 'A', startMs: 6000, endMs: 6500 }
    ]);
  });

  it('spans multiple decks and normalizes an inverted drag', () => {
    expect(marqueeTargets(rows, blocksFor, { x0: 65, x1: 45, y0: 150, y1: 10 }, xToMs)).toEqual([
      { deck: 'A', startMs: 5000, endMs: 6000 },
      { deck: 'A', startMs: 6000, endMs: 6500 },
      { deck: 'B', startMs: 5500, endMs: 6500 }
    ]);
  });

  it('selects nothing when the rect misses every block', () => {
    expect(marqueeTargets(rows, blocksFor, { x0: 20, x1: 40, y0: 10, y1: 60 }, xToMs)).toEqual([]);
  });
});

describe('mergeSelectionRanges', () => {
  it('merges overlapping and touching spans per deck, keeping decks apart', () => {
    expect(
      mergeSelectionRanges([
        { deck: 'A', startMs: 2000, endMs: 3000 },
        { deck: 'A', startMs: 1000, endMs: 2000 },
        { deck: 'A', startMs: 5000, endMs: 6000 },
        { deck: 'B', startMs: 1500, endMs: 2500 }
      ])
    ).toEqual([
      { deck: 'A', startMs: 1000, endMs: 3000 },
      { deck: 'A', startMs: 5000, endMs: 6000 },
      { deck: 'B', startMs: 1500, endMs: 2500 }
    ]);
  });
});
