import { describe, it, expect } from 'vitest';

import {
  computeRowLayout,
  ghostSpan,
  clipGestureDeltaSec,
  selectionSpanFor
} from '../timelineLayout';
import { ROW_H } from '../timelineDraw';
import type { Clip } from '@renderer/utils/types';

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
      [{ deckId: 'A', laneHeights: [{ key: 'filter', height: 64 }] }],
      0,
      120
    );
    expect(rows[0].waveformHeight).toBe(120);
    expect(rows[0].lanes[0].top).toBe(120); // lane sits directly below the waveform
    expect(rows[0].height).toBe(120 + 64);
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

describe('selectionSpanFor', () => {
  const clips = [
    clip({ blockId: 1, sessionStartMs: 1000, sessionEndMs: 2000 }),
    clip({ blockId: 1, sessionStartMs: 2000, sessionEndMs: 3000 }),
    clip({ blockId: 2, sessionStartMs: 5000, sessionEndMs: 6000 }),
    clip({ deck: 'B', blockId: 1, sessionStartMs: 0, sessionEndMs: 9000 })
  ];

  it('spans the whole block when no iteration is pinned', () => {
    expect(selectionSpanFor({ deck: 'A', blockId: 1, iterationStartMs: null }, clips, 'A')).toEqual(
      { startMs: 1000, endMs: 3000 }
    );
  });

  it('spans a single iteration when pinned', () => {
    expect(selectionSpanFor({ deck: 'A', blockId: 1, iterationStartMs: 2000 }, clips, 'A')).toEqual(
      { startMs: 2000, endMs: 3000 }
    );
  });

  it('returns null when the selection targets another deck or nothing matches', () => {
    expect(selectionSpanFor({ deck: 'B', blockId: 1, iterationStartMs: null }, clips, 'A')).toBe(
      null
    );
    expect(selectionSpanFor(null, clips, 'A')).toBe(null);
    expect(selectionSpanFor({ deck: 'A', blockId: 9, iterationStartMs: null }, clips, 'A')).toBe(
      null
    );
  });
});
