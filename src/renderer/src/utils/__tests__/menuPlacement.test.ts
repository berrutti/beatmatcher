import { describe, it, expect } from 'vitest';
import { MENU_VIEWPORT_MARGIN_PX, clampMenuPosition } from '@renderer/utils/menuPlacement';

const VIEWPORT = { width: 1000, height: 800 };
const MENU = { width: 200, height: 300 };

describe('clampMenuPosition', () => {
  it('leaves a menu that already fits where it was asked for', () => {
    expect(clampMenuPosition({ x: 100, y: 200 }, MENU, VIEWPORT)).toEqual({ x: 100, y: 200 });
  });

  it('lifts a menu whose bottom would fall off screen', () => {
    const { y } = clampMenuPosition({ x: 100, y: 700 }, MENU, VIEWPORT);
    expect(y + MENU.height).toBe(VIEWPORT.height - MENU_VIEWPORT_MARGIN_PX);
  });

  it('pulls a menu whose right edge would fall off screen', () => {
    const { x } = clampMenuPosition({ x: 950, y: 100 }, MENU, VIEWPORT);
    expect(x + MENU.width).toBe(VIEWPORT.width - MENU_VIEWPORT_MARGIN_PX);
  });

  it('keeps the top-left corner reachable when the menu is taller than the window', () => {
    const { x, y } = clampMenuPosition({ x: 100, y: 700 }, { width: 200, height: 900 }, VIEWPORT);
    expect(y).toBe(MENU_VIEWPORT_MARGIN_PX);
    expect(x).toBe(100);
  });

  it('never places a menu off the top or left', () => {
    expect(clampMenuPosition({ x: -50, y: -50 }, MENU, VIEWPORT)).toEqual({
      x: MENU_VIEWPORT_MARGIN_PX,
      y: MENU_VIEWPORT_MARGIN_PX
    });
  });
});
