import { WAVEFORM_BACKGROUND, type Rgb, type WaveformPaint } from '@renderer/utils/waveformImage';

const BASELINE: Rgb = [60, 60, 60];

export type WaveformScales = Omit<WaveformPaint, 'palette'>;

// Headroom, so a beat line still crosses black where the waveform clamps.
const STRIP_HEIGHT_FRACTION = 0.78;

export const STRIP_SCALES: WaveformScales = {
  ampScale: 0.85,
  bandScale: 0.55,
  maxBarFraction: STRIP_HEIGHT_FRACTION,
  background: null,
  baseline: BASELINE
};

export const OVERVIEW_SCALES: WaveformScales = {
  ampScale: 0.85,
  bandScale: 0.55,
  maxBarFraction: 1,
  background: WAVEFORM_BACKGROUND,
  baseline: BASELINE
};

export const EDIT_SCALES: WaveformScales = {
  ampScale: 0.9,
  bandScale: 0.55,
  maxBarFraction: 1,
  background: WAVEFORM_BACKGROUND,
  baseline: BASELINE
};
