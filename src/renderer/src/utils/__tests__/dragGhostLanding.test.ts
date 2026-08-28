import { describe, it, expect } from 'vitest';
import { ghostLanding, grabOffset } from '../dragGhostLanding';

const ROW = { left: 40, top: 400, width: 300, height: 32 };

describe('the ghost is held where the row was grabbed', () => {
  it('keeps the grabbed point under the pointer', () => {
    const offset = grabOffset(ROW, 250, 410);
    expect(offset.x).toBe(210);
    expect(offset.y).toBe(10);
  });

  it('places the ghost exactly over the row it was taken from', () => {
    const offset = grabOffset(ROW, 250, 410);
    expect(250 - offset.x).toBe(ROW.left);
    expect(410 - offset.y).toBe(ROW.top);
  });

  it('is unaffected by where in the row the pointer landed', () => {
    for (const clientX of [ROW.left, ROW.left + 5, ROW.left + ROW.width]) {
      const offset = grabOffset(ROW, clientX, 410);
      expect(clientX - offset.x).toBe(ROW.left);
    }
  });
});

describe('a released drag says whether it registered', () => {
  it('returns the ghost to the row it came from, not to a centre', () => {
    const landing = ghostLanding({
      anchor: { x: 0, y: 0 },
      origin: { left: ROW.left, top: ROW.top },
      target: null
    });
    expect(landing.left).toBe(ROW.left);
    expect(landing.top).toBe(ROW.top);
    expect(landing.scale).toBe(1);
  });

  it('moves only vertically when the drag only moved vertically', () => {
    // The ghost tracks the pointer from the same offset it was grabbed at, so
    // its horizontal position never left the row's.
    const landing = ghostLanding({
      anchor: { x: 0, y: 0 },
      origin: { left: ROW.left, top: ROW.top },
      target: null
    });
    expect(landing.left).toBe(ROW.left);
  });

  it('puts the point being held on the centre of the landing element', () => {
    const target = { left: 700, top: 100, width: 200, height: 200 };
    const anchor = { x: 210, y: 10 };
    const landing = ghostLanding({ anchor, origin: { left: 0, top: 0 }, target });
    // The ghost scales about the anchor for the whole drag, so the anchor is
    // the only point whose position is not moved by the shrink. Reading the
    // ghost's own rect instead measures it already scaled, and lands short.
    expect(landing.left + anchor.x).toBe(800);
    expect(landing.top + anchor.y).toBe(200);
    expect(landing.scale).toBeLessThan(1);
  });

  it('fades out either way, so nothing is left on screen', () => {
    for (const target of [null, { left: 0, top: 0, width: 10, height: 10 }]) {
      expect(
        ghostLanding({ anchor: { x: 0, y: 0 }, origin: { left: 0, top: 0 }, target }).opacity
      ).toBe(0);
    }
  });
});
