import { describe, it, expect } from 'vitest';
import { ALL_LANE_KEYS, LANE_DISPLAY } from '@renderer/utils/types';

describe('the timeline owns how a lane is drawn', () => {
  it('gives every editable lane a label and a row, plus the wheel', () => {
    for (const key of [...ALL_LANE_KEYS, 'jog'] as const) {
      expect(LANE_DISPLAY[key]?.shortLabel, `no label for ${key}`).toBeTruthy();
      expect(LANE_DISPLAY[key]?.group).toBeGreaterThanOrEqual(0);
    }
  });

  it('keeps the three eq lanes on one row and gain off the filter row', () => {
    expect(LANE_DISPLAY.eqLow.group).toBe(LANE_DISPLAY.eqHigh.group);
    expect(LANE_DISPLAY.eqMid.group).toBe(LANE_DISPLAY.eqHigh.group);
    expect(LANE_DISPLAY.gain.group).not.toBe(LANE_DISPLAY.filter.group);
  });

  it('gives each lane its own label, so a row cannot be mistaken for another', () => {
    const labels = Object.values(LANE_DISPLAY).map((entry) => entry.shortLabel);
    expect(new Set(labels).size).toBe(labels.length);
  });
});
