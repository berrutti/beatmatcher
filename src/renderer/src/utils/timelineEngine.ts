// A tiny retained-mode rendering engine over the 2D canvas. The session timeline
// is described as a flat list of `SceneItem`s; each item knows its bounds, draws
// itself (clipped to those bounds by the engine, so a stroke can never spill past
// where the item lives), and hit-tests itself. All interaction is reported up as
// semantic `Hit`s; items hold no gesture state.
//
// Two orderings, kept separate and explicit so neither is an accident of the
// other:
//   - DRAW order is the list order (the scene builder composes it deliberately,
//     earlier items painted under later ones).
//   - HIT precedence is NOT the draw order. When several items claim the same
//     point (a nudge under a filter region, an edge handle over a body, ...),
//     `hitScene` picks the highest-priority claimer per a caller-supplied
//     `priorityOf(hit)` table, ties broken by draw order (top-most). The
//     priority table lives in the timeline domain, not here.

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
// Built by useTimelineView; passed to every item so they never read view state
// directly.
export type ViewContext = {
  view: ViewWindow;
  scrollY: number;
  canvasW: number;
  canvasH: number;
  trackW: number;
  msToX: (ms: number) => number;
  xToMs: (x: number) => number;
  // Canvas-y of where the (scrolled) deck rows begin, and the band they occupy.
  laneOriginY: number;
  scrollViewport: { top: number; bottom: number };
};

export interface SceneItem {
  // The rectangle the item occupies in canvas pixels. The engine clips drawing
  // to it; hit-tests use the item's own hitTest (which may be tighter).
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

// The highest-priority item that claims the point. Every in-bounds item is
// hit-tested; among the claimers, the one whose `Hit` ranks highest under
// `priorityOf` wins, with draw order (later = on top) breaking ties. `priorityOf`
// is supplied by the caller and typically keys on `target` AND `part`, so the
// precedence can differ per region of an element (e.g. an edge handle outranks
// a body). Default: no priority, so ties fall back to top-most drawn.
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
