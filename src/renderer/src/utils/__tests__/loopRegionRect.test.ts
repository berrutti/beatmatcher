import { describe, it, expect } from 'vitest';
import { loopRegionRect } from '../loopRegionRect';

const tenPxPerSec = (sec: number) => sec * 10;

describe('loopRegionRect', () => {
  it('returns null when there is no region', () => {
    expect(loopRegionRect(tenPxPerSec, null, 200)).toBeNull();
  });

  it('maps start/end seconds through xFor', () => {
    const rect = loopRegionRect(tenPxPerSec, { startSec: 2, endSec: 5 }, 200);
    expect(rect).toEqual({ startX: 20, endX: 50 });
  });

  it('clamps the start to the left edge when the region begins off-screen', () => {
    const xFor = (sec: number) => (sec - 10) * 10;
    const rect = loopRegionRect(xFor, { startSec: 5, endSec: 12 }, 200);
    expect(rect?.startX).toBe(0);
  });

  it('clamps the end to the right edge when the region extends off-screen', () => {
    const xFor = (sec: number) => sec * 100;
    const rect = loopRegionRect(xFor, { startSec: 1, endSec: 3 }, 150);
    expect(rect?.endX).toBe(150);
  });

  it('returns null once the clamped region collapses to nothing', () => {
    // Entirely to the left of the visible window.
    const xFor = (sec: number) => (sec - 100) * 10;
    const rect = loopRegionRect(xFor, { startSec: 1, endSec: 2 }, 200);
    expect(rect).toBeNull();
  });
});
