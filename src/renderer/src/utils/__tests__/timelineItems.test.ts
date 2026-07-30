import { describe, it, expect } from 'vitest';
import {
  filterSelectionItem,
  clipBandItem,
  blockAtPoint,
  laneSurfaceItem,
  masterItem,
  waveformSeparatorItem,
  overviewItem,
  readOverviewHit
} from '@renderer/utils/timelineItems';
import {
  LABEL_W,
  laneValuePad,
  MASTER_GAIN_INSET_Y,
  type RowLayout,
  type SublaneLayout
} from '@renderer/utils/timelineDraw';
import type { ViewContext } from '@renderer/utils/timelineEngine';
import { MASTER_ROW_ID, type Clip } from '@renderer/utils/types';

// ms -> x as identity-ish; pt.x in these tests is given directly in ms units.
const vc = { msToX: (ms: number) => ms, trackW: 10_000 } as ViewContext;

function clipAt(deck: string, startMs: number, endMs: number, loop = false): Clip {
  return {
    deck,
    sessionStartMs: startMs,
    sessionEndMs: endMs,
    trackPath: '/t/a.mp3',
    trackName: 'a',
    trackStartSec: 0,
    playbackRate: 1,
    blockId: 1,
    loop: loop ? { startSec: 0, endSec: 1 } : null,
    waveSegments: [],
    bpm: null,
    beatOffsetSec: null
  };
}

describe('filterSelectionItem', () => {
  it('insets the outline so its bottom border clears the row divider', () => {
    const lane: SublaneLayout = { key: 'filter', top: 100, height: 80 };
    const rects: { x: number; y: number; w: number; h: number }[] = [];
    const ctx = {
      strokeStyle: '',
      lineWidth: 0,
      strokeRect: (x: number, y: number, w: number, h: number) => rects.push({ x, y, w, h })
    } as unknown as CanvasRenderingContext2D;

    filterSelectionItem(lane, 200, 600).draw(ctx, vc);

    expect(rects).toHaveLength(1);
    const [r] = rects;
    const pad = laneValuePad(lane.height);
    expect(r.y).toBe(lane.top + pad);
    expect(r.h).toBe(lane.height - 2 * pad);
    // The divider is drawn at the lane bottom; the border must stay above it.
    expect(r.y + r.h).toBeLessThan(lane.top + lane.height);
  });
});

// The gesture takes the value rect straight from the hit, so an item that reports
// its frame instead puts drawn points where they are not rendered.
describe('lane hit-test', () => {
  it('reports the deck lane value area, not its frame', () => {
    const lane: SublaneLayout = { key: 'filter', top: 100, height: 80 };
    const hit = laneSurfaceItem(lane, 'A', undefined).hitTest({ x: LABEL_W + 10, y: 140 }, vc);
    const pad = laneValuePad(lane.height);
    expect(hit?.target).toBe('lane');
    expect(hit?.data).toEqual({ top: lane.top + pad, height: lane.height - 2 * pad });
  });

  it('reports the master row as a lane so it draws like a deck lane', () => {
    const masterLanes = { gain: [], xfader: [] };
    const item = masterItem(200, 20, masterLanes, 'masterGain');
    const hit = item.hitTest({ x: LABEL_W + 10, y: 210 }, vc);
    expect(hit?.target).toBe('lane');
    expect(hit?.deck).toBe(MASTER_ROW_ID);
    expect(hit?.part).toBe('masterGain');
    expect(hit?.data).toEqual({
      top: 200 + MASTER_GAIN_INSET_Y,
      height: 20 - 2 * MASTER_GAIN_INSET_Y
    });
  });

  it('keeps the master label column on the dropdown', () => {
    const item = masterItem(200, 20, { gain: [], xfader: [] }, 'xfader');
    expect(item.hitTest({ x: LABEL_W - 5, y: 210 }, vc)?.target).toBe('laneDropdown');
  });
});

describe('clip hit-test', () => {
  const row = {
    deckId: 'A',
    top: 0,
    height: 84,
    waveformHeight: 80,
    lanes: []
  } as unknown as RowLayout;
  const clips = [clipAt('A', 1000, 5000)];
  const item = clipBandItem(row, clips, [], new Map(), '#fff', true, []);

  it('reports a trim edge only within the grab tolerance, body otherwise', () => {
    expect(item.hitTest({ x: 1000, y: 10 }, vc)?.part).toBe('start');
    expect(item.hitTest({ x: 5000, y: 10 }, vc)?.part).toBe('end');
    expect(item.hitTest({ x: 3000, y: 10 }, vc)?.part).toBe('body');
  });

  it('never reports a falsy/empty part for the body (the bug that broke clip drag)', () => {
    const hit = item.hitTest({ x: 3000, y: 10 }, vc);
    expect(hit?.part).toBeTruthy();
    expect(['start', 'end', 'body']).toContain(hit?.part);
  });

  it('loop blocks expose no trim edges (they move whole)', () => {
    const loopClips = [clipAt('A', 1000, 5000, true)];
    expect(blockAtPoint(loopClips, 'A', 1000, vc)?.edge).toBeNull();
    expect(blockAtPoint(loopClips, 'A', 5000, vc)?.edge).toBeNull();
  });

  it('returns null left of the label column', () => {
    expect(item.hitTest({ x: LABEL_W - 1, y: 10 }, vc)).toBeNull();
  });
});

describe('waveformSeparatorItem', () => {
  const row = {
    deckId: 'A',
    top: 0,
    height: 200,
    waveformHeight: 80,
    lanes: []
  } as unknown as RowLayout;
  const separator = waveformSeparatorItem(row, 'A');

  it('grabs a thin band at the waveform/lane boundary', () => {
    expect(separator.hitTest({ x: 100, y: 80 }, vc)?.target).toBe('waveformSeparator');
    expect(separator.hitTest({ x: 100, y: 82 }, vc)?.target).toBe('waveformSeparator');
    expect(separator.hitTest({ x: 100, y: 90 }, vc)).toBeNull();
  });
});

describe('readOverviewHit', () => {
  const overviewVc = {
    canvasW: 1000,
    canvasH: 400,
    trackW: 800,
    view: { start: 200, duration: 300 },
    msToX: (ms: number) => ms
  } as ViewContext;

  it('reads back every hit the overview item produces', () => {
    const item = overviewItem(1000, [], 0, {});

    for (let x = 0; x <= 1000; x++) {
      const hit = item.hitTest({ x, y: 380 }, overviewVc);
      if (!hit) continue;
      const overview = readOverviewHit(hit);
      expect(overview, `x=${x}`).not.toBeNull();
      expect(typeof overview?.frac, `x=${x}`).toBe('number');
      expect(Number.isFinite(overview?.frac), `x=${x}`).toBe(true);
    }
  });

  it('rejects a hit that is not the overview, or carries no fraction', () => {
    expect(readOverviewHit({ target: 'clip', part: 'move', data: 0.5 })).toBeNull();
    expect(readOverviewHit({ target: 'overview', part: 'move' })).toBeNull();
    expect(readOverviewHit({ target: 'overview', part: 'move', data: 'half' })).toBeNull();
    expect(readOverviewHit({ target: 'overview', part: 'move', data: NaN })).toBeNull();
    expect(readOverviewHit({ target: 'overview', part: 'wobble', data: 0.5 })).toBeNull();
  });
});
