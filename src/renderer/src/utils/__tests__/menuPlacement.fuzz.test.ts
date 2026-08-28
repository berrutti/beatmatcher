import { describe, it, expect } from 'vitest';
import { MENU_VIEWPORT_MARGIN_PX, clampMenuPosition } from '@renderer/utils/menuPlacement';

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

describe('menu placement under fuzzed anchors and window sizes', () => {
  it('keeps a menu that fits fully on screen', () => {
    const random = makeRandom(5);
    for (let step = 0; step < 3000; step++) {
      const viewport = { width: 320 + random() * 2200, height: 240 + random() * 1400 };
      const size = { width: random() * 400, height: random() * 600 };
      const desired = {
        x: (random() - 0.2) * viewport.width,
        y: (random() - 0.2) * viewport.height
      };

      const placed = clampMenuPosition(desired, size, viewport);
      if (size.width + 2 * MENU_VIEWPORT_MARGIN_PX > viewport.width) continue;
      if (size.height + 2 * MENU_VIEWPORT_MARGIN_PX > viewport.height) continue;

      expect(placed.x).toBeGreaterThanOrEqual(MENU_VIEWPORT_MARGIN_PX);
      expect(placed.y).toBeGreaterThanOrEqual(MENU_VIEWPORT_MARGIN_PX);
      expect(placed.x + size.width).toBeLessThanOrEqual(viewport.width - MENU_VIEWPORT_MARGIN_PX);
      expect(placed.y + size.height).toBeLessThanOrEqual(viewport.height - MENU_VIEWPORT_MARGIN_PX);
    }
  });

  it('keeps the top-left corner reachable even when the menu cannot fit', () => {
    const random = makeRandom(13);
    for (let step = 0; step < 1000; step++) {
      const viewport = { width: 200 + random() * 400, height: 200 + random() * 400 };
      const size = {
        width: viewport.width + random() * 800,
        height: viewport.height + random() * 800
      };
      const desired = { x: random() * 2000 - 500, y: random() * 2000 - 500 };

      const placed = clampMenuPosition(desired, size, viewport);

      expect(placed.x).toBe(MENU_VIEWPORT_MARGIN_PX);
      expect(placed.y).toBe(MENU_VIEWPORT_MARGIN_PX);
    }
  });

  it('is idempotent, so re-placing an already placed menu never drifts', () => {
    const random = makeRandom(17);
    for (let step = 0; step < 2000; step++) {
      const viewport = { width: 320 + random() * 2200, height: 240 + random() * 1400 };
      const size = { width: random() * 900, height: random() * 900 };
      const desired = { x: (random() - 0.5) * 3000, y: (random() - 0.5) * 3000 };

      const once = clampMenuPosition(desired, size, viewport);
      expect(clampMenuPosition(once, size, viewport)).toEqual(once);
    }
  });
});
