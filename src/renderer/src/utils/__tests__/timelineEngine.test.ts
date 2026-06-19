import { describe, it, expect } from 'vitest';
import { hitScene, type SceneItem, type ViewContext, type Hit, type Rect } from '../timelineEngine';

const vc = {} as ViewContext;

function item(rect: Rect, hit: Hit | null): SceneItem {
  return {
    bounds: () => rect,
    draw: () => {},
    hitTest: () => hit
  };
}

describe('hitScene', () => {
  it('returns the top-most (last-drawn) item that claims the point', () => {
    const under = item({ x: 0, y: 0, w: 100, h: 100 }, { target: 'under' });
    const over = item({ x: 0, y: 0, w: 100, h: 100 }, { target: 'over' });
    expect(hitScene([under, over], { x: 50, y: 50 }, vc)?.target).toBe('over');
  });

  it('skips items whose bounds exclude the point without calling hitTest', () => {
    let called = false;
    const offscreen: SceneItem = {
      bounds: () => ({ x: 200, y: 200, w: 10, h: 10 }),
      draw: () => {},
      hitTest: () => {
        called = true;
        return { target: 'x' };
      }
    };
    expect(hitScene([offscreen], { x: 5, y: 5 }, vc)).toBeNull();
    expect(called).toBe(false);
  });

  it('falls through to a lower item when the top item is in-bounds but declines', () => {
    const accepts = item({ x: 0, y: 0, w: 100, h: 100 }, { target: 'accepts' });
    const declines = item({ x: 0, y: 0, w: 100, h: 100 }, null);
    expect(hitScene([accepts, declines], { x: 50, y: 50 }, vc)?.target).toBe('accepts');
  });

  it('returns null when nothing claims the point', () => {
    expect(hitScene([], { x: 0, y: 0 }, vc)).toBeNull();
  });

  it('lets an explicit priority override draw order', () => {
    // `nudge` is drawn first (under), `filter` last (on top), but priority puts
    // the nudge in front.
    const nudge = item({ x: 0, y: 0, w: 100, h: 100 }, { target: 'nudge' });
    const filter = item({ x: 0, y: 0, w: 100, h: 100 }, { target: 'filter' });
    const priorityOf = (h: Hit) => (h.target === 'nudge' ? 10 : 1);
    expect(hitScene([nudge, filter], { x: 50, y: 50 }, vc, priorityOf)?.target).toBe('nudge');
  });

  it('ranks by part, not just element kind', () => {
    const body = item({ x: 0, y: 0, w: 100, h: 100 }, { target: 'filter', part: 'body' });
    const edge = item({ x: 0, y: 0, w: 100, h: 100 }, { target: 'filter', part: 'edge' });
    const nudge = item({ x: 0, y: 0, w: 100, h: 100 }, { target: 'nudge' });
    const rank = (h: Hit) => {
      const key = h.part ? `${h.target}:${h.part}` : h.target;
      return { 'filter:edge': 10, nudge: 5, 'filter:body': 1 }[key] ?? 0;
    };
    // edge beats the nudge; the nudge beats the body.
    expect(hitScene([body, edge, nudge], { x: 50, y: 50 }, vc, rank)?.part).toBe('edge');
    expect(hitScene([body, nudge], { x: 50, y: 50 }, vc, rank)?.target).toBe('nudge');
  });
});
