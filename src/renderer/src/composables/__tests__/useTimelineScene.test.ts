import { describe, it, expect, vi } from 'vitest';
import type { SceneItem, ViewContext } from '@renderer/utils/timelineEngine';
import type { DeckId } from '@renderer/stores/decks';
import { LABEL_W, type LaneKey } from '@renderer/utils/timelineDraw';
import type { MasterLaneKey } from '@renderer/utils/types';

// Tag the row-divider item so we can locate it in the composed scene.
vi.mock('@renderer/utils/timelineItems', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@renderer/utils/timelineItems')>();
  return {
    ...actual,
    rowDividersItem: () => ({
      __tag: 'dividers',
      bounds: () => ({ x: 0, y: 0, w: 0, h: 0 }),
      draw: () => {},
      hitTest: () => null
    })
  };
});

const { buildScene } = await import('@renderer/composables/useTimelineScene');
type SceneInput = Parameters<typeof buildScene>[0];

const vc = {
  mixerId: 'classic-3band',
  laneOriginY: 16,
  scrollViewport: { top: 0, bottom: 1000 },
  view: { start: 0, duration: 1000 },
  trackW: 100,
  canvasW: 132,
  canvasH: 1000,
  msToX: (ms: number) => ms,
  xToMs: (x: number) => x
} as ViewContext;

function input(overlays: SceneItem[], masterLane: MasterLaneKey = 'masterGain'): SceneInput {
  return {
    vc,
    decks: ['A'] as DeckId[],
    clips: [],
    loadedSpans: [],
    deckLanes: {},
    masterLanes: { gain: [], xfader: [] },
    deckNudges: {},
    waveforms: new Map(),
    playheadMs: 0,
    durationMs: 1000,
    editMode: false,
    laneFor: () => 'filter' as LaneKey,
    masterLane,
    laneHeight: 64,
    waveformHeight: 80,
    accentFor: () => '#ffffff',
    audibleFor: () => true,
    soloFor: () => false,
    mutedFor: () => false,
    clipSelection: [],
    filterSelection: null,
    overlays
  };
}

// The master row is the only way to reach a master-scope lane, so its label
// column has to open the picker and its track area has to report which lane is on
// display, the way a deck row does.
describe('the master row', () => {
  function hitsAt(items: SceneItem[], x: number, y: number) {
    return items.map((item) => item.hitTest?.({ x, y }, vc) ?? null).filter((hit) => hit !== null);
  }

  const insideMasterRow = 20;

  it('opens the lane picker from its label column', () => {
    const { items } = buildScene(input([]));

    expect(hitsAt(items, 4, insideMasterRow)).toContainEqual({
      target: 'laneDropdown',
      deck: 'master'
    });
  });

  it('reports the lane on display, not a fixed one', () => {
    const gain = buildScene(input([], 'masterGain'));
    const xfader = buildScene(input([], 'xfader'));
    const trackX = LABEL_W + 10;

    expect(hitsAt(gain.items, trackX, insideMasterRow)).toContainEqual({
      target: 'master',
      deck: 'master',
      part: 'masterGain'
    });
    expect(hitsAt(xfader.items, trackX, insideMasterRow)).toContainEqual({
      target: 'master',
      deck: 'master',
      part: 'xfader'
    });
  });
});

describe('buildScene z-order', () => {
  it('draws row dividers after gesture overlays so previews never cover the divider', () => {
    const overlay: SceneItem = {
      bounds: () => ({ x: 0, y: 0, w: 1, h: 1 }),
      draw: () => {},
      hitTest: () => null
    };
    const { items } = buildScene(input([overlay]));

    const overlayIdx = items.indexOf(overlay);
    const dividerIdx = items.findIndex((it) => (it as { __tag?: string }).__tag === 'dividers');

    expect(overlayIdx).toBeGreaterThanOrEqual(0);
    expect(dividerIdx).toBeGreaterThan(overlayIdx);
  });
});
