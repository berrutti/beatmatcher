import { describe, it, expect, vi } from 'vitest';
import type { SceneItem, ViewContext } from '@renderer/utils/timelineEngine';
import type { DeckId } from '@renderer/stores/decks';
import type { LaneKey } from '@renderer/utils/timelineDraw';

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

const vc = {
  laneOriginY: 16,
  scrollViewport: { top: 0, bottom: 1000 },
  view: { start: 0, duration: 1000 },
  trackW: 100,
  canvasW: 132,
  canvasH: 1000,
  msToX: (ms: number) => ms,
  xToMs: (x: number) => x
} as ViewContext;

function input(overlays: SceneItem[]) {
  return {
    vc,
    decks: ['A'] as DeckId[],
    clips: [],
    loadedSpans: [],
    deckLanes: {},
    masterLanes: { gain: [] },
    deckNudges: {},
    waveforms: new Map(),
    playheadMs: 0,
    durationMs: 1000,
    editMode: false,
    laneFor: () => 'filter' as LaneKey,
    laneHeight: 64,
    waveformHeight: 80,
    accentFor: () => '#ffffff',
    audibleFor: () => true,
    soloFor: () => false,
    mutedFor: () => false,
    clipSelection: [],
    filterSelection: null,
    scrollY: 0,
    maxScrollY: 0,
    overlays
  };
}

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
