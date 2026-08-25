import type { ViewWindow } from '@renderer/utils/timelineView';

export type Point = { x: number; y: number };
export type Rect = { x: number; y: number; w: number; h: number };

// What a point landed on. `target` is the item kind (e.g. 'filterRegion'),
// `part` the sub-region within it (e.g. 'start' | 'end' | 'body'), and `data`
// the item's payload the gesture/controller needs (the span, clip, etc.).
export type Hit = {
  target: string;
  deck?: string;
  part?: string;
  data?: unknown;
};

// The per-frame projection every item draws and hit-tests against: the camera.
// Built by useTimelineView. Passed to every item so they never read view state
// directly.
export type ViewContext = {
  view: ViewWindow;
  scrollY: number;
  canvasW: number;
  canvasH: number;
  trackW: number;
  msToX: (ms: number) => number;
  xToMs: (x: number) => number;
  // The open session's mixer. Lane ranges, labels and units are read from it,
  // so it travels with the projection rather than being looked up per drawer.
  mixerId: string;
  // Canvas-y of where the (scrolled) deck rows begin, and the band they occupy.
  laneOriginY: number;
  scrollViewport: { top: number; bottom: number };
};

export interface SceneItem {
  // The rectangle the item occupies in canvas pixels. The engine clips drawing
  // to it. Hit-tests use the item's own hitTest (which may be tighter).
  bounds(vc: ViewContext): Rect;
  draw(ctx: CanvasRenderingContext2D, vc: ViewContext): void;
  hitTest(pt: Point, vc: ViewContext): Hit | null;
}

function pointInRect(pt: Point, r: Rect): boolean {
  return pt.x >= r.x && pt.x <= r.x + r.w && pt.y >= r.y && pt.y <= r.y + r.h;
}

// Draw every item in order (later items paint on top), each clipped to its own
// bounds so nothing bleeds past where it lives.
export function renderScene(
  ctx: CanvasRenderingContext2D,
  items: SceneItem[],
  vc: ViewContext
): void {
  for (const item of items) {
    const bounds = item.bounds(vc);
    ctx.save();
    ctx.beginPath();
    ctx.rect(bounds.x, bounds.y, bounds.w, bounds.h);
    ctx.clip();
    item.draw(ctx, vc);
    ctx.restore();
  }
}

// Ties fall back to the top-most drawn item.
export function hitScene(
  items: SceneItem[],
  pt: Point,
  vc: ViewContext,
  priorityOf: (hit: Hit) => number = () => 0
): Hit | null {
  let best: Hit | null = null;
  let bestRank = -Infinity;
  for (const item of items) {
    if (!pointInRect(pt, item.bounds(vc))) continue;
    const hit = item.hitTest(pt, vc);
    if (!hit) continue;
    const rank = priorityOf(hit);
    // >= so that, at equal priority, later (top-most) items win the tie.
    if (rank >= bestRank) {
      bestRank = rank;
      best = hit;
    }
  }
  return best;
}
