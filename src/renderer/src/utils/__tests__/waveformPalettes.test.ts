import { describe, it, expect } from 'vitest';
import { waveformPalette } from '../waveformPalettes';

const FLAT: [number, number, number] = [1, 1, 1];

describe('waveformPalette', () => {
  it('gives the blended styles the balance and the stacked ones colours', () => {
    expect(waveformPalette('blended', [1, 2, 4])).toEqual({ kind: 'blended', balance: [1, 2, 4] });
    expect(waveformPalette('threeBand', FLAT).kind).toBe('stacked');
    expect(waveformPalette('twoTone', FLAT).kind).toBe('stacked');
  });

  it('gives three band seven distinct colours', () => {
    const palette = waveformPalette('threeBand', FLAT);
    if (palette.kind !== 'stacked') throw new Error('expected a stacked palette');
    const distinct = new Set(Object.values(palette.colors).map((rgb) => rgb.join(',')));
    expect(distinct.size).toBe(7);
  });

  it('gives two tone only a body and a highs colour', () => {
    const palette = waveformPalette('twoTone', FLAT);
    if (palette.kind !== 'stacked') throw new Error('expected a stacked palette');
    const distinct = new Set(Object.values(palette.colors).map((rgb) => rgb.join(',')));
    expect(distinct.size).toBe(2);
  });
});
