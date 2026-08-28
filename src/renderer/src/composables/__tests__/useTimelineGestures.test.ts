import { describe, it, expect } from 'vitest';
import { useTimelineGestures, type GestureDeps } from '@renderer/composables/useTimelineGestures';
import type { SceneItem, Hit, ViewContext } from '@renderer/utils/timelineEngine';
import type { BpmContext, Intent } from '@renderer/utils/timelineIntents';
import type { Clip, DeckLanes } from '@renderer/utils/types';
import { LABEL_W } from '@renderer/utils/timelineDraw';

function fakeItem(hit: Hit | null): SceneItem {
  return {
    bounds: () => ({ x: 0, y: 0, w: 1000, h: 1000 }),
    draw: () => {},
    hitTest: () => hit
  };
}

function cursorFor(hit: Hit | null, editMode = true): string {
  const deps: GestureDeps = {
    camera: {} as GestureDeps['camera'],
    emit: () => {},
    getItems: () => (hit ? [fakeItem(hit)] : []),
    getRows: () => [],
    getVc: () => ({}) as ViewContext,
    getClips: () => [],
    getEvents: () => [],
    getDeckLanes: () => ({}),
    laneHeightFor: () => 64,
    waveformHeightFor: () => 80,
    isEditMode: () => editMode,
    durationMs: () => 1000,
    accentFor: () => '#ffffff',
    requestRender: () => {},
    setCursor: () => {}
  };
  return useTimelineGestures(deps).cursorFor({ x: 50, y: 50 });
}

describe('cursorFor', () => {
  it('shows grab on a clip body and ew-resize only on its trim edges (edit mode)', () => {
    expect(cursorFor({ target: 'clip', part: 'body' })).toBe('grab');
    expect(cursorFor({ target: 'clip', part: 'start' })).toBe('ew-resize');
    expect(cursorFor({ target: 'clip', part: 'end' })).toBe('ew-resize');
  });

  it('shows no clip cursor outside edit mode', () => {
    expect(cursorFor({ target: 'clip', part: 'body' }, false)).toBe('');
    expect(cursorFor({ target: 'clip', part: 'start' }, false)).toBe('');
  });

  it('shows grab on a filter region body and ew-resize on its edges', () => {
    expect(cursorFor({ target: 'filterRegion', part: 'body' })).toBe('grab');
    expect(cursorFor({ target: 'filterRegion', part: 'start' })).toBe('ew-resize');
    expect(cursorFor({ target: 'filterRegion', part: 'end' })).toBe('ew-resize');
  });

  it('falls back to the default cursor with nothing under the pointer', () => {
    expect(cursorFor(null)).toBe('');
  });
});

const VIEW_CONTEXT = {
  view: { start: 0, duration: 1000 },
  scrollY: 0,
  canvasW: 1000,
  canvasH: 1000,
  trackW: 800,
  msToX: (ms: number) => ms,
  xToMs: (x: number) => x,
  mixerId: 'classic-3band',
  laneOriginY: 0,
  scrollViewport: { top: 0, bottom: 1000 }
} as ViewContext;

function gestureHarness(
  hit: Hit,
  clips: Clip[] = [],
  view = { start: 0, duration: 1000 },
  deckLanes: Record<string, DeckLanes> = {}
) {
  const intents: Intent[] = [];
  const cursors: string[] = [];
  const deps: GestureDeps = {
    camera: {
      currentView: () => view,
      panByPixels: () => {},
      panByMsDelta: () => {},
      zoomAt: () => {},
      maxScrollY: () => 0
    } as unknown as GestureDeps['camera'],
    emit: (intent) => intents.push(intent),
    getItems: () => [fakeItem(hit)],
    getRows: () => [],
    getVc: () => VIEW_CONTEXT,
    getClips: () => clips,
    getEvents: () => [],
    getDeckLanes: () => deckLanes,
    laneHeightFor: () => 64,
    waveformHeightFor: () => 80,
    isEditMode: () => true,
    durationMs: () => 1000,
    accentFor: () => '#ffffff',
    requestRender: () => {},
    setCursor: (cursor) => cursors.push(cursor)
  };
  return { gestures: useTimelineGestures(deps), intents, cursors };
}

const RECT = { left: 0, top: 0 } as DOMRect;
const mouseAt = (x: number, y: number) => ({ clientX: x, clientY: y, button: 0 }) as MouseEvent;
const rightMouseAt = (x: number, y: number) =>
  ({ clientX: x, clientY: y, button: 2 }) as MouseEvent;

describe('drag detection across a full mousedown/move/up/click sequence', () => {
  it('a vertical-only lane resize does not seek or clear the selection', () => {
    const { gestures, intents } = gestureHarness({
      target: 'laneSeparator',
      deck: 'A',
      data: 'filter'
    });
    gestures.onMouseDown(mouseAt(400, 100), RECT);
    gestures.onMouseMove(mouseAt(400, 160), RECT);
    gestures.onMouseUp();
    gestures.onClick(mouseAt(400, 160), RECT);

    expect(intents.map((intent) => intent.type)).toEqual(['lane.resize', 'lane.resize']);
  });

  it('a vertical-only waveform resize does not seek or clear the selection', () => {
    const { gestures, intents } = gestureHarness({ target: 'waveformSeparator', deck: 'A' });
    gestures.onMouseDown(mouseAt(400, 100), RECT);
    gestures.onMouseMove(mouseAt(400, 160), RECT);
    gestures.onMouseUp();
    gestures.onClick(mouseAt(400, 160), RECT);

    expect(intents.map((intent) => intent.type)).toEqual(['waveform.resize', 'waveform.resize']);
  });

  it('a click with no movement still seeks', () => {
    const { gestures, intents } = gestureHarness({ target: 'clipBand', deck: 'A', data: {} });
    gestures.onMouseDown(mouseAt(400, 100), RECT);
    gestures.onMouseUp();
    gestures.onClick(mouseAt(400, 100), RECT);

    expect(intents.map((intent) => intent.type)).toContain('seek');
  });
});

describe('filter region edge', () => {
  const span = { startMs: 100, endMs: 500 };

  it('a click on the edge selects without resizing', () => {
    const { gestures, intents } = gestureHarness({
      target: 'filterRegion',
      deck: 'A',
      part: 'start',
      data: span
    });
    gestures.onMouseDown(mouseAt(104, 50), RECT);
    gestures.onMouseUp();
    gestures.onClick(mouseAt(104, 50), RECT);

    expect(intents.map((intent) => intent.type)).not.toContain('filterRegion.resize');
  });

  it('an actual drag on the edge resizes', () => {
    const { gestures, intents } = gestureHarness({
      target: 'filterRegion',
      deck: 'A',
      part: 'start',
      data: span
    });
    gestures.onMouseDown(mouseAt(104, 50), RECT);
    gestures.onMouseMove(mouseAt(200, 50), RECT);
    gestures.onMouseUp();

    expect(intents).toContainEqual({
      type: 'filterRegion.resize',
      deck: 'A',
      span,
      edge: 'start',
      newMs: 200
    });
  });
});

describe('the tempo context under a right-click', () => {
  const BOUNDARY_MS = 1000;

  function clipAt(startMs: number, endMs: number, bpm: number | null): Clip {
    return {
      blockId: startMs,
      bpm,
      waveSegments: [],
      beatOffsetSec: 0,
      deck: 'A',
      loop: null,
      playbackRate: 1,
      sessionStartMs: startMs,
      sessionEndMs: endMs,
      trackName: 'name',
      trackPath: '/track.mp3',
      trackStartSec: 0
    };
  }

  function bpmFromMenuAt(clips: Clip[], ms: number): BpmContext | null {
    const block = { deck: 'A', startMs: clips[0].sessionStartMs, endMs: clips[0].sessionEndMs };
    const { gestures, intents } = gestureHarness(
      { target: 'clip', deck: 'A', data: { block, rowTop: 0 } },
      clips
    );
    gestures.onContextMenu(mouseAt(ms, 10), RECT);
    const menu = intents.find((intent) => intent.type === 'menu.deck');
    return menu?.type === 'menu.deck' ? menu.bpm : null;
  }

  it('takes the grid from a later clip when an earlier one has none', () => {
    const context = bpmFromMenuAt(
      [clipAt(0, BOUNDARY_MS, null), clipAt(BOUNDARY_MS, 2000, 128)],
      BOUNDARY_MS
    );

    expect(context?.trackBpm).toBe(128);
  });

  it('reports no tempo when nothing under the pointer has a grid', () => {
    expect(bpmFromMenuAt([clipAt(0, BOUNDARY_MS, null)], 500)).toBeNull();
  });
});

describe('overview drag from outside the viewport', () => {
  it('recentres on the pointer and keeps the zoom level', () => {
    const { gestures, intents } = gestureHarness(
      { target: 'overview', part: 'outside', data: 0.1 },
      [],
      { start: 0, duration: 200 }
    );

    gestures.onMouseDown(mouseAt(100, 5), RECT);

    const views = intents.filter((intent) => intent.type === 'view.set');
    const first = views[0];
    if (first.type !== 'view.set') throw new Error('expected a view.set');
    // data 0.1 of a 1000ms session is 100ms, centred in a 200ms window and then
    // clamped to the session start, at the zoom the press started from.
    expect(first.view.duration).toBe(200);
    expect(first.view.start).toBe(0);
  });

  it('drags by how far the pointer moved, not to where it landed', () => {
    const { gestures, intents } = gestureHarness(
      { target: 'overview', part: 'outside', data: 0.1 },
      [],
      { start: 0, duration: 200 }
    );

    gestures.onMouseDown(mouseAt(100, 5), RECT);
    gestures.onMouseMove(mouseAt(500, 5), RECT);

    const views = intents.filter((intent) => intent.type === 'view.set');
    expect(views).toHaveLength(2);
    const last = views[views.length - 1];
    if (last.type !== 'view.set') throw new Error('expected a view.set');
    // x=500 is (500 - LABEL_W) / 800 = 0.585, so the pointer travelled
    // 0.585 - 0.1 = 0.485 of a 1000ms session from where it was pressed.
    expect(last.view.duration).toBe(200);
    expect(last.view.start).toBe(485);
  });
});

describe('overview drag grabbed off-centre', () => {
  // Pressed at 0.5 of the session with the window covering 0..200ms, so the
  // pointer sits far to the right of the rectangle's own centre.
  const grabbed = { target: 'overview', part: 'move', data: 0.5 } as const;

  it('does not snap the rectangle centre to the pointer on the first move', () => {
    const { gestures, intents } = gestureHarness(grabbed, [], { start: 0, duration: 200 });

    gestures.onMouseDown(mouseAt(432, 5), RECT);
    gestures.onMouseMove(mouseAt(440, 5), RECT);

    const views = intents.filter((intent) => intent.type === 'view.set');
    const last = views[views.length - 1];
    if (last.type !== 'view.set') throw new Error('expected a view.set');
    // x=440 is 0.51 of the session, 0.01 past the press: a 10ms nudge, not a
    // jump to a window centred on 500ms.
    expect(last.view.start).toBeCloseTo(10, 6);
    expect(last.view.duration).toBe(200);
  });

  it('holds the grab offset across a multi-step drag', () => {
    const { gestures, intents } = gestureHarness(grabbed, [], { start: 0, duration: 200 });

    gestures.onMouseDown(mouseAt(432, 5), RECT);
    for (const x of [500, 600, 700, 640]) gestures.onMouseMove(mouseAt(x, 5), RECT);

    const views = intents.filter((intent) => intent.type === 'view.set');
    const last = views[views.length - 1];
    if (last.type !== 'view.set') throw new Error('expected a view.set');
    // Every move is measured from the press, so the last one alone decides:
    // x=640 is 0.76, which is 0.26 past the press.
    expect(last.view.start).toBeCloseTo(260, 6);
    expect(last.view.duration).toBe(200);
  });

  it('leaves the view where it was when the pointer returns to the press point', () => {
    const { gestures, intents } = gestureHarness(grabbed, [], { start: 300, duration: 200 });

    gestures.onMouseDown(mouseAt(432, 5), RECT);
    gestures.onMouseMove(mouseAt(600, 5), RECT);
    gestures.onMouseMove(mouseAt(432, 5), RECT);

    const views = intents.filter((intent) => intent.type === 'view.set');
    const last = views[views.length - 1];
    if (last.type !== 'view.set') throw new Error('expected a view.set');
    expect(last.view.start).toBeCloseTo(300, 6);
  });
});

describe('the lane picker anchors to the lane, not to the pointer', () => {
  it('opens beside the label column at the lane top, wherever the click landed', () => {
    const { gestures, intents } = gestureHarness({
      target: 'laneDropdown',
      deck: 'A',
      data: { top: 180, height: 80 }
    });
    gestures.onMouseDown(mouseAt(34, 290), { left: 30, top: 50 } as DOMRect);

    const [intent] = intents;
    expect(intent).toMatchObject({ type: 'lane.openDropdown', deck: 'A' });
    if (intent.type !== 'lane.openDropdown') throw new Error('wrong intent');
    expect(intent.clientX).toBeGreaterThan(30 + LABEL_W);
    expect(intent.clientY).toBe(50 + 180);
  });
});

describe('the right button opens menus rather than arming a gesture', () => {
  it('arms nothing on the clip band, so a right-click cannot rubber-band a selection', () => {
    const { gestures, intents } = gestureHarness({ target: 'clipBand', deck: 'A', data: {} });
    gestures.onMouseDown(rightMouseAt(400, 100), RECT);
    gestures.onMouseMove(mouseAt(600, 140), RECT);

    expect(gestures.overlays()).toEqual([]);
    expect(intents).toEqual([]);
  });
});

describe('the deck label opens the deck menu', () => {
  it('opens beside the label column at the row top, like the lane picker', () => {
    const { gestures, intents } = gestureHarness({
      target: 'deckLabel',
      deck: 'A',
      data: { top: 120 }
    });
    gestures.onMouseDown(mouseAt(34, 200), { left: 30, top: 50 } as DOMRect);

    const [intent] = intents;
    expect(intent).toMatchObject({ type: 'menu.deck', deck: 'A', bpm: null, split: null });
    if (intent.type !== 'menu.deck') throw new Error('wrong intent');
    expect(intent.clientX).toBeGreaterThan(30 + LABEL_W);
    expect(intent.clientY).toBe(50 + 120);
  });
});

describe('clicking an active filter span selects it', () => {
  function lanesWithSpan(): Record<string, DeckLanes> {
    return {
      A: {
        gain: [],
        eqLow: [],
        eqMid: [],
        eqHigh: [],
        filter: [],
        rate: [],
        rateMin: 0.9,
        rateMax: 1.1,
        filterActive: [{ startMs: 200, endMs: 600 }]
      }
    };
  }

  it('selects from anywhere inside the span, not only its grab bar', () => {
    const { gestures, intents } = gestureHarness(
      { target: 'lane', deck: 'A', part: 'filter', data: { top: 0, height: 60 } },
      [],
      { start: 0, duration: 1000 },
      lanesWithSpan()
    );
    gestures.onClick(mouseAt(400, 30), RECT);

    expect(intents.map((intent) => intent.type)).toContain('filterRegion.select');
  });

  it('clears the selection when the click lands outside every span', () => {
    const { gestures, intents } = gestureHarness(
      { target: 'lane', deck: 'A', part: 'filter', data: { top: 0, height: 60 } },
      [],
      { start: 0, duration: 1000 },
      lanesWithSpan()
    );
    gestures.onClick(mouseAt(800, 30), RECT);

    expect(intents.map((intent) => intent.type)).toContain('filterRegion.clearSelection');
  });

  it('leaves another lane alone', () => {
    const { gestures, intents } = gestureHarness(
      { target: 'lane', deck: 'A', part: 'gain', data: { top: 0, height: 60 } },
      [],
      { start: 0, duration: 1000 },
      lanesWithSpan()
    );
    gestures.onClick(mouseAt(400, 30), RECT);

    expect(intents.map((intent) => intent.type)).not.toContain('filterRegion.select');
  });
});

describe('the right button belongs to the lane, not its chrome', () => {
  const rightClick = (x: number, y: number) =>
    ({ clientX: x, clientY: y, button: 2 }) as MouseEvent;

  it('opens no menu from the lane label or its dropdown', () => {
    for (const target of ['laneDropdown', 'deckLabel'] as const) {
      const { gestures, intents } = gestureHarness({ target, deck: 'A', data: { top: 100 } });
      gestures.onContextMenu(rightClick(20, 120), RECT);
      expect(intents, target).toEqual([]);
    }
  });

  it('opens no menu from a separator', () => {
    for (const target of ['laneSeparator', 'waveformSeparator'] as const) {
      const { gestures, intents } = gestureHarness({ target, deck: 'A', data: 'filter' });
      gestures.onContextMenu(rightClick(400, 120), RECT);
      expect(intents, target).toEqual([]);
    }
  });

  it('still opens the deck menu on the lane itself', () => {
    const { gestures, intents } = gestureHarness({
      target: 'lane',
      deck: 'A',
      part: 'filter',
      data: { top: 0, height: 60 }
    });
    gestures.onContextMenu(rightClick(400, 30), RECT);

    expect(intents.map((intent) => intent.type)).toEqual(['menu.deck']);
  });
});

describe('the master row has lanes of its own', () => {
  it('opens the menu on its lane, naming the lane so a reset can reach it', () => {
    const { gestures, intents } = gestureHarness({
      target: 'lane',
      deck: 'master',
      part: 'masterGain',
      data: { top: 0, height: 30 }
    });
    gestures.onContextMenu({ clientX: 400, clientY: 20, button: 2 } as MouseEvent, RECT);

    const [intent] = intents;
    expect(intent).toMatchObject({ type: 'menu.deck', deck: 'master' });
    if (intent.type !== 'menu.deck') throw new Error('wrong intent');
    expect(intent.lane?.key).toBe('masterGain');
  });
});
