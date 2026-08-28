import { describe, it, expect } from 'vitest';
import { drawLaneWaveform, LABEL_W, BEAT_LINE_W } from '@renderer/utils/timelineDraw';
import type { Clip, WaveSegment } from '@renderer/utils/types';

type Fill = { x: number; y: number; w: number; h: number };

const LANE_Y = 200;
const LANE_H = 60;
const VIEW_MS = 4000;
const TRACK_W = 400;
const msToX = (ms: number) => LABEL_W + (ms / VIEW_MS) * TRACK_W;

// 120 bpm from a fractional offset, so the beats never land on whole pixels by luck.
function clip(): Clip {
  return {
    deck: 'A',
    blockId: 1,
    sessionStartMs: 0,
    sessionEndMs: 4000,
    trackStartSec: 0,
    trackPath: '/a.mp3',
    trackName: 'a',
    bpm: 120,
    beatOffsetSec: 0.137,
    loopRegion: null,
    waveSegments: [
      { wallStartMs: 0, wallEndMs: 4000, trackStartSec: 0, trackEndSec: 4 }
    ] as WaveSegment[]
  } as unknown as Clip;
}

function fills(): Fill[] {
  const out: Fill[] = [];
  const ctx = {
    fillRect: (x: number, y: number, w: number, h: number) => out.push({ x, y, w, h }),
    save: () => {},
    restore: () => {},
    beginPath: () => {},
    rect: () => {},
    clip: () => {},
    canvas: { clientWidth: LABEL_W + TRACK_W + 12 },
    set globalAlpha(_v: number) {},
    set fillStyle(_v: string) {}
  } as unknown as CanvasRenderingContext2D;
  const waveforms = new Map([
    ['/a.mp3', { startSec: 0, endSec: 10, amps: new Float32Array(200).fill(0.8) }]
  ]);
  drawLaneWaveform(ctx, [clip()], waveforms, msToX, LANE_Y, LANE_H);
  return out;
}

const beatLines = (all: Fill[]) => all.filter((fill) => fill.w === BEAT_LINE_W);

describe('beat lines under a lane waveform', () => {
  it('draws them, so a lane can be read against the grid', () => {
    expect(beatLines(fills()).length).toBeGreaterThan(0);
  });

  it('lands each on a whole pixel, or a line spreads over two at half strength', () => {
    for (const line of beatLines(fills())) {
      expect(Number.isInteger(line.x), `line at ${line.x} is off-pixel`).toBe(true);
    }
  });

  it('is thick enough to read', () => {
    expect(BEAT_LINE_W).toBeGreaterThan(1);
  });

  it('spans only the band the waveform occupies, not the whole lane', () => {
    const lines = beatLines(fills());

    expect(lines.every((line) => line.y > LANE_Y)).toBe(true);
    expect(lines.every((line) => line.y + line.h < LANE_Y + LANE_H)).toBe(true);
  });
});
