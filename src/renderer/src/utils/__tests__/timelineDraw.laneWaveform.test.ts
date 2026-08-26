import { describe, it, expect } from 'vitest';
import { drawLaneWaveform, LABEL_W } from '@renderer/utils/timelineDraw';
import type { Clip, WaveSegment } from '@renderer/utils/types';

type Bar = { x: number; y: number; w: number; h: number };

const LANE_Y = 200;
const LANE_H = 60;
const VIEW_MS = 4000;
const TRACK_W = 400;

const msToX = (ms: number) => LABEL_W + (ms / VIEW_MS) * TRACK_W;

// Two abutting pieces whose join lands on a fractional pixel, which is what a
// rate or nudge change produces.
function splitClip(startMs: number, endMs: number): Clip {
  const midMs = (startMs + endMs) / 2 + 3.7;
  const base = clip(startMs, endMs);
  return {
    ...base,
    waveSegments: [
      {
        wallStartMs: startMs,
        wallEndMs: midMs,
        trackStartSec: startMs / 1000,
        trackEndSec: midMs / 1000
      },
      {
        wallStartMs: midMs,
        wallEndMs: endMs,
        trackStartSec: midMs / 1000,
        trackEndSec: endMs / 1000
      }
    ] as WaveSegment[]
  } as unknown as Clip;
}

function clip(startMs: number, endMs: number): Clip {
  return {
    deck: 'A',
    blockId: 1,
    sessionStartMs: startMs,
    sessionEndMs: endMs,
    trackStartSec: 0,
    trackPath: '/a.mp3',
    trackName: 'a',
    bpm: null,
    beatOffsetSec: 0,
    loopRegion: null,
    waveSegments: [
      {
        wallStartMs: startMs,
        wallEndMs: endMs,
        trackStartSec: startMs / 1000,
        trackEndSec: endMs / 1000
      }
    ] as WaveSegment[]
  } as unknown as Clip;
}

const WAVEFORM = {
  startSec: 0,
  endSec: 10,
  amps: new Float32Array(Array.from({ length: 200 }, () => 0.8))
};

function draw(clips: Clip[], waveform = WAVEFORM): Bar[] {
  const bars: Bar[] = [];
  const ctx = {
    fillRect: (x: number, y: number, w: number, h: number) => bars.push({ x, y, w, h }),
    save: () => {},
    restore: () => {},
    beginPath: () => {},
    rect: () => {},
    clip: () => {},
    canvas: { clientWidth: LABEL_W + TRACK_W + 12 },
    set globalAlpha(_v: number) {},
    set fillStyle(_v: string) {}
  } as unknown as CanvasRenderingContext2D;
  const waveforms = new Map([['/a.mp3', waveform]]);
  drawLaneWaveform(ctx, clips, waveforms, msToX, LANE_Y, LANE_H);
  return bars;
}

describe('drawLaneWaveform', () => {
  it('draws the deck waveform inside the lane, so every lane shares one scale', () => {
    const bars = draw([clip(0, 2000)]);

    expect(bars.length).toBeGreaterThan(0);
    expect(bars.every((bar) => bar.y >= LANE_Y)).toBe(true);
    expect(bars.every((bar) => bar.y + bar.h <= LANE_Y + LANE_H)).toBe(true);
  });

  it('places a clip at the same x its clip band would, so the lanes line up', () => {
    const bars = draw([clip(1000, 3000)]);
    const left = Math.min(...bars.map((bar) => bar.x));

    expect(left).toBeGreaterThanOrEqual(msToX(1000) - 1);
  });

  it('leaves no seam where two wave segments join', () => {
    const columns = draw([splitClip(0, 3000)])
      .map((bar) => bar.x)
      .sort((left, right) => left - right);

    expect(new Set(columns).size, 'a column was drawn twice').toBe(columns.length);
    for (const x of columns) expect(Number.isInteger(x), `column at ${x} is off-pixel`).toBe(true);
    const gaps = columns.filter((x, i) => i > 0 && x !== columns[i - 1] + 1);
    expect(gaps, 'a column was skipped between segments').toEqual([]);
  });

  it('draws nothing for a lane with no clip under it', () => {
    expect(draw([])).toHaveLength(0);
  });

  it('draws nothing when the track has no waveform loaded yet', () => {
    expect(
      draw([clip(0, 2000)], { startSec: 0, endSec: 0, amps: new Float32Array() })
    ).toHaveLength(0);
  });
});
