import { describe, it, expect } from 'vitest';
import { buildWaveformImageData } from '../waveformImage';

// ImageData is a browser API not available in the Node test environment.
if (typeof ImageData === 'undefined') {
  (globalThis as Record<string, unknown>).ImageData = class {
    data: Uint8ClampedArray;
    constructor(public width: number, public height: number) {
      this.data = new Uint8ClampedArray(width * height * 4);
    }
  };
}

const BG = 10; // background channel value

function pixel(img: ImageData, col: number, row: number) {
  const i = (row * img.width + col) * 4;
  return { r: img.data[i], g: img.data[i + 1], b: img.data[i + 2], a: img.data[i + 3] };
}

function isBackground(img: ImageData, col: number, row: number): boolean {
  const p = pixel(img, col, row);
  return p.r === BG && p.g === BG && p.b === BG && p.a === 255;
}

describe('buildWaveformImageData', () => {
  it('fills the entire image with the background color', () => {
    const peaks = new Float32Array(8); // 2 silent points
    const img = buildWaveformImageData(4, 10, peaks, 0.9);
    for (let col = 0; col < img.width; col++) {
      for (let row = 0; row < img.height; row++) {
        expect(isBackground(img, col, row)).toBe(true);
      }
    }
  });

  it('leaves silent columns (amp < 0.001) as background', () => {
    const peaks = new Float32Array([0.5, 0.5, 0.5, 0.0]); // color but no amplitude
    const img = buildWaveformImageData(1, 10, peaks, 1.0);
    for (let row = 0; row < img.height; row++) {
      expect(isBackground(img, 0, row)).toBe(true);
    }
  });

  it('draws a centered bar whose height reflects sqrt of amplitude', () => {
    // amp = 1.0 → displayAmp = sqrt(1.0) = 1.0
    // halfCh = 5, ampScale = 1.0 → barPx = floor(1.0 * 5 * 1.0) = 5
    // yTop = max(0, 5-5) = 0, yBot = min(10, 5+5) = 10 → all rows colored
    const peaks = new Float32Array([0, 0, 1, 1.0]);
    const img = buildWaveformImageData(1, 10, peaks, 1.0);
    for (let row = 0; row < 10; row++) {
      expect(isBackground(img, 0, row)).toBe(false);
    }
  });

  it('bar height is shorter for lower amplitude (sqrt curve)', () => {
    // amp = 0.25 → displayAmp = sqrt(0.25) = 0.5
    // halfCh = 5, ampScale = 1.0 → barPx = floor(0.5 * 5) = 2
    // yTop = max(0, 5-2) = 3, yBot = min(10, 5+2) = 7 → rows 3..6 colored
    const peaks = new Float32Array([1, 0, 0, 0.25]);
    const img = buildWaveformImageData(1, 10, peaks, 1.0);
    expect(isBackground(img, 0, 0)).toBe(true);
    expect(isBackground(img, 0, 2)).toBe(true);
    expect(isBackground(img, 0, 3)).toBe(false);
    expect(isBackground(img, 0, 6)).toBe(false);
    expect(isBackground(img, 0, 7)).toBe(true);
  });

  it('color is the amplitude-weighted average of source points in the window', () => {
    // 2 source points map to 1 output column:
    // point 0: r=1, g=0, b=0, amp=0.5 (red)
    // point 1: r=0, g=1, b=0, amp=0.5 (green)
    // expected: r = sumR/sumAmp * 255 = 0.5/1.0 * 255 = 127
    //           g = 127, b = 0
    const peaks = new Float32Array([
      1.0, 0.0, 0.0, 0.5,
      0.0, 1.0, 0.0, 0.5,
    ]);
    const img = buildWaveformImageData(1, 10, peaks, 1.0);
    // Find a colored row (middle)
    const p = pixel(img, 0, 5);
    expect(p.r).toBe(127);
    expect(p.g).toBe(127);
    expect(p.b).toBe(0);
  });

  it('uses mean aggregation, not nearest-neighbor', () => {
    // 2 source points map to 1 column: first has amp=1.0, second is silent.
    // Nearest-neighbor would pick index 0 (amp=1.0) → full-height bar.
    // Mean: avgAmp = 0.5 → displayAmp = sqrt(0.5) ≈ 0.707
    //   halfCh=5, ampScale=1 → barPx = floor(0.707*5) = 3
    //   yTop = max(0, 5-3) = 2 → rows 0 and 1 stay background
    const peaks = new Float32Array([
      0.0, 0.0, 1.0, 1.0, // loud blue
      0.0, 0.0, 0.0, 0.0, // silent
    ]);
    const img = buildWaveformImageData(1, 10, peaks, 1.0);
    expect(isBackground(img, 0, 0)).toBe(true);
    expect(isBackground(img, 0, 1)).toBe(true);
    expect(isBackground(img, 0, 2)).toBe(false); // bar starts here
  });

  it('ampScale controls bar height proportionally', () => {
    const peaks = new Float32Array([1, 0, 0, 1.0]);
    const tallImg = buildWaveformImageData(1, 20, peaks, 1.0);
    const shortImg = buildWaveformImageData(1, 20, peaks, 0.5);
    // Count colored rows for each
    let tallRows = 0, shortRows = 0;
    for (let row = 0; row < 20; row++) {
      if (!isBackground(tallImg, 0, row)) tallRows++;
      if (!isBackground(shortImg, 0, row)) shortRows++;
    }
    expect(tallRows).toBeGreaterThan(shortRows);
  });

  it('returns correct dimensions', () => {
    const peaks = new Float32Array(40); // 10 points
    const img = buildWaveformImageData(8, 16, peaks, 0.9);
    expect(img.width).toBe(8);
    expect(img.height).toBe(16);
  });
});
