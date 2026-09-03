import type { Rgb, StackedPalette, WaveformPalette } from '@renderer/utils/waveformImage';
import type { WaveformStyleOption } from '@renderer/utils/types';

export function blendedRgb(balance: [number, number, number]): WaveformPalette {
  return { kind: 'blended', balance };
}

export const BLENDED_RGB_FLAT = blendedRgb([1, 1, 1]);

// Sampled off a rendered stacked waveform rather than chosen: the overlaps are the part
// that cannot be derived, and bassMid in particular is darker than either band alone.
const THREE_BAND: StackedPalette = {
  bass: [0x00, 0x55, 0xe1],
  mid: [0xff, 0xa6, 0x00],
  high: [0xff, 0xff, 0xff],
  bassMid: [0xb4, 0x69, 0x0a],
  bassHigh: [0xd2, 0xdc, 0xfa],
  midHigh: [0xff, 0xf0, 0xd7],
  all: [0xf5, 0xeb, 0xd7]
};

export const STACKED_THREE_BAND: WaveformPalette = { kind: 'stacked', colors: THREE_BAND };

// One hue plus white highs: the mid rides with the bass, so only two colours ever show.
export function twoTone(body: Rgb, highs: Rgb): WaveformPalette {
  return {
    kind: 'stacked',
    colors: {
      bass: body,
      mid: body,
      high: highs,
      bassMid: body,
      bassHigh: highs,
      midHigh: highs,
      all: highs
    }
  };
}

const TWO_TONE_BODY: Rgb = [0x00, 0x55, 0xe1];
const TWO_TONE_HIGHS: Rgb = [0xff, 0xff, 0xff];

export function waveformPalette(
  style: WaveformStyleOption,
  balance: [number, number, number]
): WaveformPalette {
  if (style === 'threeBand') return STACKED_THREE_BAND;
  if (style === 'twoTone') return twoTone(TWO_TONE_BODY, TWO_TONE_HIGHS);
  return blendedRgb(balance);
}
