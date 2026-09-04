import { describe, it, expect } from 'vitest';
import {
  spectralColor,
  waveformColumns,
  waveformImageData,
  maxBarTop,
  bandHalfHeights,
  stackedRowColor,
  WAVEFORM_BACKGROUND,
  type WaveformPaint
} from '../waveformImage';
import { waveformPalette } from '../waveformPalettes';

// ImageData is a browser API not available in the Node test environment.
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

const BG = 10; // background channel value

const FLAT_BLENDED = waveformPalette('blended', [1, 1, 1]);
const THREE_BAND = waveformPalette('threeBand', [1, 1, 1]);

const OPAQUE: WaveformPaint = {
  ampScale: 1.0,
  bandScale: 1.0,
  maxBarFraction: 1,
  background: WAVEFORM_BACKGROUND,
  baseline: null,
  palette: FLAT_BLENDED
};
const CLAMPED: WaveformPaint = {
  ampScale: 1.5,
  bandScale: 1.0,
  maxBarFraction: 0.5,
  background: null,
  baseline: null,
  palette: FLAT_BLENDED
};

function styleWith(ampScale: number): WaveformPaint {
  return { ...OPAQUE, ampScale };
}

function pixel(img: ImageData, col: number, row: number) {
  const i = (row * img.width + col) * 4;
  return { r: img.data[i], g: img.data[i + 1], b: img.data[i + 2], a: img.data[i + 3] };
}

function isBackground(img: ImageData, col: number, row: number): boolean {
  const p = pixel(img, col, row);
  return p.r === BG && p.g === BG && p.b === BG && p.a === 255;
}

function image(
  width: number,
  height: number,
  peaks: Float32Array,
  paint: WaveformPaint
): ImageData {
  return waveformImageData(width, height, waveformColumns(peaks, width), paint);
}

describe('spectralColor', () => {
  it('gives a band on its own its primary', () => {
    expect(spectralColor(0.4, 0, 0, 1)).toEqual([255, 0, 0]);
    expect(spectralColor(0, 0.02, 0, 1)).toEqual([0, 255, 0]);
    expect(spectralColor(0, 0, 0.7, 1)).toEqual([0, 0, 255]);
  });

  it('gives three equal bands white', () => {
    expect(spectralColor(0.3, 0.3, 0.3, 1)).toEqual([255, 255, 255]);
  });

  it('takes hue from the balance between bands, not from amplitude', () => {
    const loud = spectralColor(0.6, 0.3, 0.1, 1);
    const quiet = spectralColor(0.6, 0.3, 0.1, 0.25);
    expect(loud[1] / loud[0]).toBeCloseTo(quiet[1] / quiet[0], 2);
    expect(loud[2] / loud[0]).toBeCloseTo(quiet[2] / quiet[0], 2);
  });

  it('dims with amplitude', () => {
    const loud = spectralColor(0.6, 0.3, 0.1, 1);
    const quiet = spectralColor(0.6, 0.3, 0.1, 0.25);
    expect(quiet[0]).toBeLessThan(loud[0]);
  });

  it('stays visible at zero amplitude', () => {
    expect(spectralColor(0.6, 0.3, 0.1, 0)[0]).toBe(77);
  });

  it('gives a bandless column black', () => {
    expect(spectralColor(0, 0, 0, 1)).toEqual([0, 0, 0]);
  });
});

describe('waveformColumns', () => {
  it('weights the bands by amplitude so a loud point sets the colour of its window', () => {
    const peaks = new Float32Array([1, 0, 0, 1.0, 0, 1, 0, 0.0]);
    expect(waveformColumns(peaks, 1)).toEqual([{ bass: 1, mid: 0, high: 0, amp: 0.5 }]);
  });

  it('takes the mean amplitude across the window, not the nearest point', () => {
    const peaks = new Float32Array([0, 0, 1, 1.0, 0, 0, 1, 0.0]);
    expect(waveformColumns(peaks, 1)[0].amp).toBe(0.5);
  });

  it('reads only the requested point range', () => {
    const peaks = new Float32Array([1, 0, 0, 1.0, 0, 0, 1, 1.0]);
    expect(waveformColumns(peaks, 1, 1, 2)).toEqual([{ bass: 0, mid: 0, high: 1, amp: 1 }]);
  });

  it('returns one entry per column asked for', () => {
    const peaks = new Float32Array(40);
    expect(waveformColumns(peaks, 8)).toHaveLength(8);
  });

  it('returns nothing for an empty range', () => {
    expect(waveformColumns(new Float32Array(40), 0)).toEqual([]);
    expect(waveformColumns(new Float32Array(40), 4, 3, 3)).toEqual([]);
  });
});

describe('waveformImageData', () => {
  it('fills the entire image with the background color', () => {
    const img = image(4, 10, new Float32Array(8), OPAQUE);
    for (let col = 0; col < img.width; col++) {
      for (let row = 0; row < img.height; row++) {
        expect(isBackground(img, col, row)).toBe(true);
      }
    }
  });

  it('leaves silent columns as background', () => {
    const img = image(1, 10, new Float32Array([0.5, 0.5, 0.5, 0.0]), OPAQUE);
    for (let row = 0; row < img.height; row++) {
      expect(isBackground(img, 0, row)).toBe(true);
    }
  });

  it('draws a centered bar whose height reflects sqrt of amplitude', () => {
    const img = image(1, 10, new Float32Array([0, 0, 1, 1.0]), OPAQUE);
    for (let row = 0; row < 10; row++) {
      expect(isBackground(img, 0, row)).toBe(false);
    }
  });

  it('bar height is shorter for lower amplitude (sqrt curve)', () => {
    const img = image(1, 10, new Float32Array([1, 0, 0, 0.25]), OPAQUE);
    expect(isBackground(img, 0, 0)).toBe(true);
    expect(isBackground(img, 0, 2)).toBe(true);
    expect(isBackground(img, 0, 3)).toBe(false);
    expect(isBackground(img, 0, 6)).toBe(false);
    expect(isBackground(img, 0, 7)).toBe(true);
  });

  it('color is the amplitude-weighted average of source points in the window', () => {
    const peaks = new Float32Array([1.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.5]);
    const p = pixel(image(1, 10, peaks, OPAQUE), 0, 5);
    expect(p.r).toBe(203);
    expect(p.g).toBe(203);
    expect(p.b).toBe(0);
  });

  it('uses mean aggregation, not nearest-neighbor', () => {
    const peaks = new Float32Array([0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    const img = image(1, 10, peaks, OPAQUE);
    expect(isBackground(img, 0, 0)).toBe(true);
    expect(isBackground(img, 0, 1)).toBe(true);
    expect(isBackground(img, 0, 2)).toBe(false);
  });

  it('ampScale controls bar height proportionally', () => {
    const peaks = new Float32Array([1, 0, 0, 1.0]);
    const tall = image(1, 20, peaks, styleWith(1.0));
    const short = image(1, 20, peaks, styleWith(0.5));
    let tallRows = 0,
      shortRows = 0;
    for (let row = 0; row < 20; row++) {
      if (!isBackground(tall, 0, row)) tallRows++;
      if (!isBackground(short, 0, row)) shortRows++;
    }
    expect(tallRows).toBeGreaterThan(shortRows);
  });

  it('leaves the headroom maxBarFraction reserves, however loud the column', () => {
    const img = image(1, 100, new Float32Array([1, 0, 0, 1.0]), CLAMPED);
    expect(pixel(img, 0, 0).a).toBe(0);
    expect(pixel(img, 0, 24).a).toBe(0);
    expect(pixel(img, 0, 26).a).toBe(255);
    expect(pixel(img, 0, 74).a).toBe(255);
    expect(pixel(img, 0, 99).a).toBe(0);
  });

  it('returns correct dimensions', () => {
    const img = image(8, 16, new Float32Array(40), OPAQUE);
    expect(img.width).toBe(8);
    expect(img.height).toBe(16);
  });
});

describe('maxBarTop', () => {
  it('reports the top of the tallest bar, not the top of the canvas', () => {
    const peaks = new Float32Array([1, 0, 0, 0.25, 1, 0, 0, 0.01]);
    const columns = waveformColumns(peaks, 2);
    expect(maxBarTop(columns, 10, OPAQUE)).toBe(3);
  });

  it('reports the centre line when every column is silent', () => {
    const columns = waveformColumns(new Float32Array(8), 2);
    expect(maxBarTop(columns, 10, OPAQUE)).toBe(5);
  });
});

describe('bandHalfHeights', () => {
  const column = { bass: 1, mid: 0.25, high: 0.0625, amp: 1 };

  it('takes each band from its own level, on the sqrt curve', () => {
    expect(bandHalfHeights(column, 100, OPAQUE)).toEqual([50, 25, 12]);
  });

  it('leaves a band its own headroom, so a loud one still grows', () => {
    const louder = { ...column, bass: 0.25, mid: 1 };
    expect(bandHalfHeights(louder, 100, OPAQUE)[1]).toBeGreaterThan(
      bandHalfHeights(column, 100, OPAQUE)[1]
    );
  });

  it('clamps a band at the headroom the style reserves', () => {
    expect(bandHalfHeights({ bass: 9, mid: 0, high: 0, amp: 1 }, 100, CLAMPED)[0]).toBe(25);
  });

  it('gives a silent column nothing', () => {
    expect(bandHalfHeights({ bass: 1, mid: 1, high: 1, amp: 0 }, 100, OPAQUE)).toEqual([0, 0, 0]);
  });

  it('gives a bandless column nothing', () => {
    expect(bandHalfHeights({ bass: 0, mid: 0, high: 0, amp: 1 }, 100, OPAQUE)).toEqual([0, 0, 0]);
  });
});

describe('stackedRowColor', () => {
  const colors = {
    bass: [1, 1, 1] as [number, number, number],
    mid: [2, 2, 2] as [number, number, number],
    high: [3, 3, 3] as [number, number, number],
    bassMid: [4, 4, 4] as [number, number, number],
    bassHigh: [5, 5, 5] as [number, number, number],
    midHigh: [6, 6, 6] as [number, number, number],
    all: [7, 7, 7] as [number, number, number]
  };

  it('picks the entry for whichever bands reach the row', () => {
    const halves: [number, number, number] = [30, 20, 10];
    expect(stackedRowColor(colors, halves, 5)).toEqual(colors.all);
    expect(stackedRowColor(colors, halves, 15)).toEqual(colors.bassMid);
    expect(stackedRowColor(colors, halves, 25)).toEqual(colors.bass);
  });

  it('names each pair by the bands in it, whichever is taller', () => {
    expect(stackedRowColor(colors, [0, 20, 10], 15)).toEqual(colors.mid);
    expect(stackedRowColor(colors, [10, 0, 20], 15)).toEqual(colors.high);
    expect(stackedRowColor(colors, [20, 0, 20], 15)).toEqual(colors.bassHigh);
    expect(stackedRowColor(colors, [0, 20, 20], 15)).toEqual(colors.midHigh);
  });

  it('returns nothing past the tallest band', () => {
    expect(stackedRowColor(colors, [30, 20, 10], 30)).toBeNull();
  });
});

describe('waveformImageData with a stacked palette', () => {
  const stacked: WaveformPaint = {
    ampScale: 1.0,
    bandScale: 1.0,
    maxBarFraction: 1,
    background: null,
    baseline: null,
    palette: THREE_BAND
  };

  it('paints the bands over each other, overlaps included', () => {
    const peaks = new Float32Array([1, 0.5, 0.25, 0.25]);
    const img = waveformImageData(1, 100, waveformColumns(peaks, 1), stacked);
    expect(pixel(img, 0, 50)).toEqual({ r: 0xf5, g: 0xeb, b: 0xd7, a: 255 });
    expect(pixel(img, 0, 80)).toEqual({ r: 0xb4, g: 0x69, b: 0x0a, a: 255 });
    expect(pixel(img, 0, 90)).toEqual({ r: 0x00, g: 0x55, b: 0xe1, a: 255 });
    expect(pixel(img, 0, 0).a).toBe(0);
  });

  it('shows only two colours for a two-tone palette', () => {
    const peaks = new Float32Array([1, 0.5, 0.25, 0.25]);
    const img = waveformImageData(1, 100, waveformColumns(peaks, 1), {
      ...stacked,
      palette: waveformPalette('twoTone', [1, 1, 1])
    });
    const shown = new Set<string>();
    for (let row = 0; row < 100; row++) {
      const p = pixel(img, 0, row);
      if (p.a === 255) shown.add(`${p.r},${p.g},${p.b}`);
    }
    expect(shown).toEqual(new Set(['0,85,225', '255,255,255']));
  });
});

describe('a blended palette with a band balance', () => {
  it('reads each band against its own average, not against the loudest', () => {
    const balanced: WaveformPaint = {
      ...OPAQUE,
      background: null,
      palette: waveformPalette('blended', [1, 2, 4])
    };
    const peaks = new Float32Array([0.4, 0.2, 0.1, 1]);
    const img = waveformImageData(1, 10, waveformColumns(peaks, 1), balanced);
    const p = pixel(img, 0, 5);
    expect(p.r).toBe(p.g);
    expect(p.g).toBe(p.b);
  });

  it('leaves the bands alone at a flat balance', () => {
    const flat: WaveformPaint = { ...OPAQUE, background: null, palette: FLAT_BLENDED };
    const peaks = new Float32Array([0.4, 0.2, 0.1, 1]);
    const img = waveformImageData(1, 10, waveformColumns(peaks, 1), flat);
    const p = pixel(img, 0, 5);
    expect(p.r).toBeGreaterThan(p.g);
    expect(p.g).toBeGreaterThan(p.b);
  });
});

describe('a column with nothing to show', () => {
  const withBaseline: WaveformPaint = { ...OPAQUE, background: null, baseline: [60, 60, 60] };

  it('draws the baseline where the track has not been analysed yet', () => {
    const img = image(1, 10, new Float32Array([0, 0, 0, 0]), withBaseline);
    expect(pixel(img, 0, 5)).toEqual({ r: 60, g: 60, b: 60, a: 255 });
    expect(pixel(img, 0, 4).a).toBe(0);
    expect(pixel(img, 0, 6).a).toBe(0);
  });

  it('is covered by the waveform wherever there is one', () => {
    const img = image(1, 10, new Float32Array([1, 0, 0, 1]), withBaseline);
    expect(pixel(img, 0, 5)).not.toEqual({ r: 60, g: 60, b: 60, a: 255 });
  });

  it('draws nothing when no baseline is asked for', () => {
    const img = image(1, 10, new Float32Array([0, 0, 0, 0]), {
      ...OPAQUE,
      background: null,
      baseline: null
    });
    expect(pixel(img, 0, 5).a).toBe(0);
  });
});
