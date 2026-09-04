import { describe, it, expect } from 'vitest';
import { STRIP_SCALES, OVERVIEW_SCALES, EDIT_SCALES, type WaveformScales } from '../waveformPaints';
import { waveformImageData, type WaveformColumn } from '../waveformImage';
import { waveformPalette } from '../waveformPalettes';

if (typeof ImageData === 'undefined') {
  (globalThis as Record<string, unknown>).ImageData = class {
    data: Uint8ClampedArray;
    constructor(
      public width: number,
      public height: number
    ) {
      this.data = new Uint8ClampedArray(width * height * 4);
    }
  };
}

const HEIGHT = 200;

// Band values are multiples of the track's own level, amplitude is raw RMS, so these are
// what a loud four-to-the-floor track actually reads at its kick, its body and its break.
const KICK: WaveformColumn = { bass: 2.0, mid: 0.6, high: 0.25, amp: 0.7 };
const BODY: WaveformColumn = { bass: 0.9, mid: 0.4, high: 0.15, amp: 0.4 };
const BREAK: WaveformColumn = { bass: 0.3, mid: 0.35, high: 0.2, amp: 0.1 };

function paintedRows(column: WaveformColumn, scales: WaveformScales, stacked: boolean): number {
  const image = waveformImageData(1, HEIGHT, [column], {
    ...scales,
    background: null,
    palette: waveformPalette(stacked ? 'threeBand' : 'blended', [1, 1, 1])
  });
  let rows = 0;
  for (let row = 0; row < HEIGHT; row++) {
    if (image.data[row * 4 + 3] === 255) rows++;
  }
  return rows;
}

describe('the two height curves agree on the same column', () => {
  it.each([
    ['a kick', KICK],
    ['the body of a track', BODY],
    ['a break', BREAK]
  ])('draws %s to a comparable height either way', (_label, column) => {
    const blended = paintedRows(column, STRIP_SCALES, false);
    const stacked = paintedRows(column, STRIP_SCALES, true);
    expect(Math.abs(blended - stacked) / Math.max(blended, stacked)).toBeLessThan(0.3);
  });

  it('never lets either curve reach the strip edge, so the beat grid stays readable', () => {
    for (const column of [KICK, BODY, BREAK]) {
      for (const stacked of [false, true]) {
        expect(paintedRows(column, STRIP_SCALES, stacked)).toBeLessThan(HEIGHT);
      }
    }
  });

  it('leaves the deck waveforms free of the strip headroom', () => {
    expect(STRIP_SCALES.maxBarFraction).toBeLessThan(1);
    expect(OVERVIEW_SCALES.maxBarFraction).toBe(1);
    expect(EDIT_SCALES.maxBarFraction).toBe(1);
  });
});

describe('a louder column is drawn taller', () => {
  it.each([
    ['blended', false],
    ['stacked', true]
  ])('holds for %s', (_label, stacked) => {
    expect(paintedRows(BREAK, STRIP_SCALES, stacked)).toBeLessThan(
      paintedRows(BODY, STRIP_SCALES, stacked)
    );
    expect(paintedRows(BODY, STRIP_SCALES, stacked)).toBeLessThan(
      paintedRows(KICK, STRIP_SCALES, stacked)
    );
  });
});
