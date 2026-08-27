import { describe, it, expect } from 'vitest';
import {
  DEFAULT_LANE_HEIGHT,
  DEFAULT_MASTER_LANE_HEIGHT,
  MIN_LANE_HEIGHT,
  DEFAULT_WAVEFORM_HEIGHT,
  MIN_WAVEFORM_HEIGHT,
  laneHeightFor,
  waveformHeightFor,
  withLaneHeight,
  withWaveformHeight
} from '@renderer/utils/laneHeights';
import { MASTER_ROW_ID } from '@renderer/utils/types';
import { SEPARATOR_GRAB_PX } from '@renderer/utils/timelineItems';

describe('laneHeightFor', () => {
  it('starts an unsized lane at the default', () => {
    expect(laneHeightFor({}, 'A', 'filter')).toBe(DEFAULT_LANE_HEIGHT);
  });

  it('starts a master lane far shorter than a deck lane', () => {
    expect(laneHeightFor({}, MASTER_ROW_ID, 'masterGain')).toBe(DEFAULT_MASTER_LANE_HEIGHT);
    expect(DEFAULT_MASTER_LANE_HEIGHT).toBeLessThan(DEFAULT_LANE_HEIGHT);
  });

  it('reads back a stored height', () => {
    expect(laneHeightFor({ 'A:filter': 90 }, 'A', 'filter')).toBe(90);
  });

  it('sizes each lane on its own, so one resize does not move the rest', () => {
    expect(laneHeightFor({ 'A:filter': 90 }, 'A', 'gain')).toBe(DEFAULT_LANE_HEIGHT);
  });

  it('sizes the same lane on another deck on its own too', () => {
    expect(laneHeightFor({ 'A:filter': 90 }, 'B', 'filter')).toBe(DEFAULT_LANE_HEIGHT);
  });

  it('lifts a stored height below the minimum, so a lane cannot be lost', () => {
    expect(laneHeightFor({ 'A:filter': 1 }, 'A', 'filter')).toBe(MIN_LANE_HEIGHT);
  });

  it('falls back to the default when the stored value is not a number', () => {
    expect(laneHeightFor({ 'A:filter': 'tall' }, 'A', 'filter')).toBe(DEFAULT_LANE_HEIGHT);
  });
});

describe('withLaneHeight', () => {
  it('clamps what it stores rather than trusting the drag', () => {
    expect(withLaneHeight({}, 'A', 'filter', 2)['A:filter']).toBe(MIN_LANE_HEIGHT);
  });

  it('leaves every other lane untouched', () => {
    expect(withLaneHeight({ 'A:gain': 90 }, 'A', 'filter', 70)).toEqual({
      'A:gain': 90,
      'A:filter': 70
    });
  });

  it('returns a new object, so a store can tell it changed', () => {
    const stored = { 'A:gain': 90 };
    expect(withLaneHeight(stored, 'A', 'filter', 70)).not.toBe(stored);
  });
});

describe('waveformHeightFor', () => {
  it('starts an unsized waveform at the default', () => {
    expect(waveformHeightFor({}, 'A')).toBe(DEFAULT_WAVEFORM_HEIGHT);
  });

  it('reads back a stored height', () => {
    expect(waveformHeightFor({ 'A:waveform': 120 }, 'A')).toBe(120);
  });

  it('sizes each deck on its own, so one resize does not move the rest', () => {
    expect(waveformHeightFor({ 'A:waveform': 120 }, 'B')).toBe(DEFAULT_WAVEFORM_HEIGHT);
  });

  it('never collides with a lane on the same deck', () => {
    const stored = withWaveformHeight(withLaneHeight({}, 'A', 'filter', 90), 'A', 120);
    expect(laneHeightFor(stored, 'A', 'filter')).toBe(90);
    expect(waveformHeightFor(stored, 'A')).toBe(120);
  });

  it('lifts a stored height below the minimum, so a waveform cannot be lost', () => {
    expect(waveformHeightFor({ 'A:waveform': 1 }, 'A')).toBe(MIN_WAVEFORM_HEIGHT);
  });

  it('falls back to the default when the stored value is not a number', () => {
    expect(waveformHeightFor({ 'A:waveform': 'tall' }, 'A')).toBe(DEFAULT_WAVEFORM_HEIGHT);
  });

  it('ignores a height left over from when one size applied to every deck', () => {
    expect(waveformHeightFor(160, 'A')).toBe(DEFAULT_WAVEFORM_HEIGHT);
  });
});

describe('withWaveformHeight', () => {
  it('clamps what it stores rather than trusting the drag', () => {
    expect(withWaveformHeight({}, 'A', 2)['A:waveform']).toBe(MIN_WAVEFORM_HEIGHT);
  });

  it('leaves every other deck untouched', () => {
    const stored = withWaveformHeight({ 'B:waveform': 120 }, 'A', 100);
    expect(stored['B:waveform']).toBe(120);
  });
});

describe('the floors keep a row usable', () => {
  it('leaves clear space between the separators bounding a lane', () => {
    expect(MIN_LANE_HEIGHT).toBeGreaterThan(SEPARATOR_GRAB_PX * 4);
    expect(MIN_WAVEFORM_HEIGHT).toBeGreaterThan(SEPARATOR_GRAB_PX * 4);
  });
});
