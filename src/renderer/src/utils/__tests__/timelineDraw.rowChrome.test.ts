import { describe, it, expect } from 'vitest';
import {
  LABEL_W,
  LANE_CARET_CLOSED,
  LANE_CARET_OPEN,
  drawDeckRowChrome,
  type LaneKey,
  type RowLayout
} from '@renderer/utils/timelineDraw';

type Fill = { style: string; x: number; y: number; w: number; h: number };
type Label = { text: string; y: number; alpha: number; clip: Rect | null };
type Rect = { x: number; y: number; w: number; h: number };

const ROW: RowLayout = {
  deckId: 'A',
  top: 100,
  height: 160,
  waveformHeight: 80,
  lanes: [{ key: 'filter', top: 180, height: 80 }]
};

type ChromeOverrides = {
  openLane?: LaneKey | null;
  badgeAlpha?: number;
  menuOpen?: boolean;
  solo?: boolean;
  muted?: boolean;
};

function drawWith(
  row: RowLayout,
  accent: string,
  overrides: ChromeOverrides = {}
): { fills: Fill[]; labels: Label[] } {
  const fills: Fill[] = [];
  const labels: Label[] = [];
  let style = '';
  let translatedY = 0;
  let alpha = 1;
  let pending: Rect | null = null;
  let clip: Rect | null = null;
  const ctx = {
    set fillStyle(value: string) {
      style = value;
    },
    get fillStyle() {
      return style;
    },
    fillRect: (x: number, y: number, w: number, h: number) => fills.push({ style, x, y, w, h }),
    fillText: (text: string) => labels.push({ text, y: translatedY, alpha, clip }),
    save: () => {},
    restore: () => {
      clip = null;
    },
    beginPath: () => {},
    rect: (x: number, y: number, w: number, h: number) => {
      pending = { x, y, w, h };
    },
    clip: () => {
      clip = pending;
    },
    translate: (_x: number, y: number) => {
      translatedY = y;
    },
    rotate: () => {},
    set globalAlpha(value: number) {
      alpha = value;
    },
    get globalAlpha() {
      return alpha;
    },
    set font(_v: string) {},
    set textAlign(_v: string) {},
    set textBaseline(_v: string) {}
  } as unknown as CanvasRenderingContext2D;

  drawDeckRowChrome(ctx, row, 500, {
    accent,
    audible: true,
    solo: overrides.solo ?? false,
    muted: overrides.muted ?? false,
    deckLabel: 'DECK A',
    badgeLabel: 'MUTE',
    badgeAlpha: overrides.badgeAlpha ?? 0,
    laneLabel: (key) => key.toUpperCase(),
    openLane: overrides.openLane ?? null,
    menuOpen: overrides.menuOpen ?? false
  });
  return { fills, labels };
}

function fillsFor(accent: string): Fill[] {
  return drawWith(ROW, accent).fills;
}

const rowFills = (fills: Fill[]) => fills.filter((fill) => fill.y === ROW.top);

describe('drawDeckRowChrome', () => {
  it('tints the row with the deck accent, so a row is identified by colour', () => {
    const tinted = rowFills(fillsFor('#3b82f6'));

    expect(tinted.some((fill) => fill.style.toLowerCase().includes('3b82f6'))).toBe(true);
  });

  it('draws no fill that depends on the row position, so nothing zebras', () => {
    const fills = rowFills(fillsFor('#3b82f6'));
    expect(new Set(fills.map((fill) => fill.style)).size).toBe(fills.length);
  });

  it('draws two accents differently, which is what separates adjacent decks', () => {
    expect(rowFills(fillsFor('#3b82f6'))).not.toEqual(rowFills(fillsFor('#f97316')));
  });
});

describe('a stacked row names every lane', () => {
  it('labels each lane at its own centre, so no lane is left unnamed', () => {
    const stacked: RowLayout = {
      deckId: 'A',
      top: 100,
      height: 260,
      waveformHeight: 80,
      lanes: [
        { key: 'filter', top: 180, height: 80 },
        { key: 'gain', top: 260, height: 100 }
      ]
    };

    const { labels } = drawWith(stacked, '#3b82f6');

    expect(labels.some((label) => label.text.includes('FILTER') && label.y === 220)).toBe(true);
    expect(labels.some((label) => label.text.includes('GAIN') && label.y === 310)).toBe(true);
  });
});

describe('the lane caret points at its menu', () => {
  const stacked: RowLayout = {
    deckId: 'A',
    top: 100,
    height: 260,
    waveformHeight: 80,
    lanes: [
      { key: 'filter', top: 180, height: 80 },
      { key: 'gain', top: 260, height: 100 }
    ]
  };

  it('turns back toward the label column while that lane is open', () => {
    const { labels } = drawWith(stacked, '#3b82f6', { openLane: 'gain' });

    expect(labels.find((label) => label.text.includes('GAIN'))?.text).toContain(LANE_CARET_OPEN);
    expect(labels.find((label) => label.text.includes('FILTER'))?.text).toContain(
      LANE_CARET_CLOSED
    );
  });
});

describe('the mute and solo badge fades', () => {
  it('draws nothing while it is fully faded out', () => {
    const { labels } = drawWith(ROW, '#3b82f6', { badgeAlpha: 0, muted: true });

    expect(labels.some((label) => label.text.includes('MUTE'))).toBe(false);
  });

  it('draws it at the fade alpha, so it arrives and leaves gradually', () => {
    const { labels } = drawWith(ROW, '#3b82f6', { badgeAlpha: 0.4, muted: true });

    expect(labels.find((label) => label.text.includes('MUTE'))?.alpha).toBeCloseTo(0.4);
  });

  it('leaves the rest of the row opaque, so only the badge fades', () => {
    const { labels } = drawWith(ROW, '#3b82f6', { badgeAlpha: 0.4, muted: true });

    expect(labels.find((label) => label.text.includes('DECK A'))?.alpha).toBe(1);
    expect(labels.find((label) => label.text.includes('FILTER'))?.alpha).toBe(1);
  });
});

describe('the deck label carries its own menu caret', () => {
  it('points at the menu, and turns back while it is open', () => {
    expect(drawWith(ROW, '#3b82f6').labels.find((l) => l.text.includes('DECK A'))?.text).toContain(
      LANE_CARET_CLOSED
    );
    expect(
      drawWith(ROW, '#3b82f6', { menuOpen: true }).labels.find((l) => l.text.includes('DECK A'))
        ?.text
    ).toContain(LANE_CARET_OPEN);
  });
});

describe('a label is cut off rather than bleeding out of a short lane', () => {
  it('clips each lane label to its own band', () => {
    const shortLane: RowLayout = {
      deckId: 'A',
      top: 100,
      height: 104,
      waveformHeight: 80,
      lanes: [{ key: 'filter', top: 180, height: 24 }]
    };

    const { labels } = drawWith(shortLane, '#3b82f6');

    expect(labels.find((label) => label.text.includes('FILTER'))?.clip).toEqual({
      x: 0,
      y: 180,
      w: LABEL_W,
      h: 24
    });
  });

  it('clips the deck label to the waveform band', () => {
    const { labels } = drawWith(ROW, '#3b82f6');

    expect(labels.find((label) => label.text.includes('DECK A'))?.clip).toEqual({
      x: 0,
      y: ROW.top,
      w: LABEL_W,
      h: ROW.waveformHeight
    });
  });
});
