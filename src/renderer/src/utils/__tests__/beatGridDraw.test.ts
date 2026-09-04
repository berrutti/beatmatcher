import { describe, it, expect } from 'vitest';
import { drawBeatMarker, EDIT_GRID, STRIP_GRID, type BeatGridWeight } from '../beatGridDraw';
import type { BeatMarkerKind } from '../beatGrid';

function markerFills(kind: BeatMarkerKind, weight: BeatGridWeight): string[] {
  const filled: string[] = [];
  const ctx = {
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 0,
    globalAlpha: 1,
    save: () => {},
    restore: () => {},
    beginPath: () => {},
    moveTo: () => {},
    lineTo: () => {},
    closePath: () => {},
    stroke: () => {},
    fill(this: { fillStyle: string }) {
      filled.push(this.fillStyle);
    }
  } as unknown as CanvasRenderingContext2D;
  drawBeatMarker(ctx, 100, 0, 50, kind, weight);
  return filled;
}

describe('drawBeatMarker', () => {
  it('gives a phrase, a bar and a beat three different colours', () => {
    const phrase = markerFills('phrase', STRIP_GRID);
    const bar = markerFills('bar', STRIP_GRID);
    const beat = markerFills('beat', STRIP_GRID);

    expect(new Set([...phrase, ...bar, ...beat]).size).toBe(3);
  });

  it('marks both edges, so the grid reads without following a line down', () => {
    expect(markerFills('phrase', EDIT_GRID).length).toBe(2);
  });

  it('draws no triangle where the weight asks for none', () => {
    expect(EDIT_GRID.beatMarkerHalfWidth).toBe(0);
    expect(markerFills('beat', EDIT_GRID)).toEqual([]);
  });
});
