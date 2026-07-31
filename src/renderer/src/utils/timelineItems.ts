// SceneItem factories: the reusable "components" the timeline composes. Each
// wraps an existing draw primitive (timelineDraw.ts) and adds a hitTest that
// reports a semantic Hit (target + part). Drawing is bounded to the item's rect
// by the engine, so nothing here guards its own edges. Items hold no gesture
// state; what to do with a hit is the gesture/controller layer's job.

import type { SceneItem, Rect, ViewContext, Hit } from '@renderer/utils/timelineEngine';
import type { RowLayout, SublaneLayout } from '@renderer/utils/timelineDraw';
import {
  LABEL_W,
  TICK_H,
  OVERVIEW_H,
  drawTickRow,
  drawDeckRowChrome,
  drawMasterRowChrome,
  drawMasterLane,
  drawDeckLanes,
  drawClip,
  drawClipBpmLabels,
  drawClipSelection,
  drawLoadedSpan,
  drawLoadedSpanLabel,
  drawNudgeSpans,
  drawOverview,
  drawPlayhead,
  drawRowDividers,
  drawFrameGutters,
  laneValuePad,
  MASTER_GAIN_INSET_Y,
  type DeckRowChrome
} from '@renderer/utils/timelineDraw';
import {
  overlapsRange,
  hitTestOverview,
  OVERVIEW_PARTS,
  type OverviewHit
} from '@renderer/utils/timelineView';
import { blocksForDeck } from '@renderer/utils/sessionCore';
import type {
  TransportBlock,
  Clip,
  LoadedSpan,
  DeckLanes,
  MasterLanes,
  MasterLaneKey,
  NudgeSpan,
  FilterActiveSpan
} from '@renderer/utils/types';
import { MASTER_ROW_ID } from '@renderer/utils/types';
import type { TrackWaveform } from '@renderer/utils/timelineDraw';

// Thin grab tolerance for edges/separators, in pixels.
const EDGE_GRAB_PX = 6;
const SEPARATOR_GRAB_PX = 3;

// The playhead's drawn line is 1px; its bounds are widened so the clip rect
// the engine sets never clips the line at the view edges.
const PLAYHEAD_HIT_W = 3;
const PLAYHEAD_HALF_W = 1;

const trackRect = (viewContext: ViewContext, top: number, height: number): Rect => ({
  x: LABEL_W,
  y: top,
  w: viewContext.trackW,
  h: height
});

export function tickRowItem(): SceneItem {
  return {
    bounds: (viewContext) => ({ x: 0, y: 0, w: viewContext.canvasW, h: TICK_H }),
    draw: (ctx, viewContext) =>
      drawTickRow(
        ctx,
        viewContext.canvasW,
        viewContext.trackW,
        viewContext.view,
        viewContext.msToX
      ),
    hitTest: () => null
  };
}

export function deckChromeItem(
  row: RowLayout,
  zebraIndex: number,
  chrome: Omit<DeckRowChrome, 'zebraIndex'>
): SceneItem {
  return {
    bounds: (viewContext) => ({ x: 0, y: row.top, w: viewContext.canvasW, h: row.height }),
    draw: (ctx, viewContext) =>
      drawDeckRowChrome(
        ctx,
        row,
        viewContext.canvasW,
        { zebraIndex, ...chrome },
        viewContext.mixerId
      ),
    hitTest: (point) => {
      // Only the label-column lane caret is interactive here.
      if (point.x > LABEL_W || row.lanes.length === 0) return null;
      const lane = row.lanes[0];
      if (point.y < lane.top || point.y > lane.top + lane.height) return null;
      return { target: 'laneDropdown', deck: row.deckId };
    }
  };
}

export function clipBandItem(
  row: RowLayout,
  clips: Clip[],
  loadedSpans: LoadedSpan[],
  waveforms: Map<string, TrackWaveform>,
  accent: string,
  audible: boolean,
  selectionSpans: { startMs: number; endMs: number }[]
): SceneItem {
  return {
    bounds: (viewContext) => ({
      x: LABEL_W,
      y: row.top,
      w: viewContext.trackW,
      h: row.waveformHeight
    }),
    draw: (ctx, viewContext) => {
      const waveformH = row.waveformHeight;
      const viewEnd = viewContext.view.start + viewContext.view.duration;
      const visible = (startMs: number, endMs: number) =>
        overlapsRange(startMs, endMs, viewContext.view.start, viewEnd);
      for (const span of loadedSpans) {
        if (span.deck === row.deckId && visible(span.startMs, span.endMs)) {
          drawLoadedSpan(ctx, span, row.top, waveformH, accent, viewContext.msToX);
        }
      }
      for (const clip of clips) {
        if (clip.deck === row.deckId && visible(clip.sessionStartMs, clip.sessionEndMs)) {
          drawClip(
            ctx,
            clip,
            waveforms.get(clip.trackPath),
            row.top,
            waveformH,
            accent,
            viewContext.msToX
          );
        }
      }
      if (!audible) {
        ctx.fillStyle = '#00000090';
        ctx.fillRect(LABEL_W, row.top, viewContext.trackW, waveformH);
      }
      // Track-name labels last so they stay legible over the waveform (and over
      // the inaudible dim).
      for (const span of loadedSpans) {
        if (span.deck === row.deckId && visible(span.startMs, span.endMs)) {
          drawLoadedSpanLabel(ctx, span, row.top, waveformH, viewContext.msToX);
        }
      }
      // Per-region BPM numbers, above the track name so each pitched/nudged
      // region reads its tempo.
      for (const clip of clips) {
        if (clip.deck === row.deckId && visible(clip.sessionStartMs, clip.sessionEndMs)) {
          drawClipBpmLabels(ctx, clip, row.top, waveformH, viewContext.msToX);
        }
      }
      for (const span of selectionSpans) {
        if (visible(span.startMs, span.endMs)) {
          drawClipSelection(ctx, span.startMs, span.endMs, row.top, waveformH, viewContext.msToX);
        }
      }
    },
    hitTest: (point, viewContext) => {
      if (point.y < row.top || point.y > row.top + row.waveformHeight || point.x < LABEL_W)
        return null;
      const block = blockAtPoint(clips, row.deckId, point.x, viewContext);
      if (block) {
        return {
          target: 'clip',
          deck: row.deckId,
          part: block.edge ?? 'body',
          data: { block: block.block, rowTop: row.top }
        };
      }
      return { target: 'clipBand', deck: row.deckId, data: { rowTop: row.top } };
    }
  };
}

// The transport block under x, plus a trim edge when near a boundary (loop
// blocks expose no edges, they move whole). Body hits are resolved first and
// the edge zone shrinks with the block, so a narrow block stays selectable
// instead of every pixel landing in an edge zone (possibly a NEIGHBOUR's,
// which turned attempted selections of tiny regions into trims of the block
// next door).
export function blockAtPoint(
  clips: Clip[],
  deck: string,
  localX: number,
  viewContext: ViewContext
): { block: TransportBlock; edge: 'start' | 'end' | null } | null {
  const blocks = blocksForDeck(clips, deck);
  for (const block of blocks) {
    const startX = viewContext.msToX(block.startMs);
    const endX = viewContext.msToX(block.endMs);
    if (localX < startX || localX > endX) continue;
    if (!block.loop) {
      const edgeGrab = Math.min(EDGE_GRAB_PX, (endX - startX) / 4);
      if (localX - startX <= edgeGrab) return { block, edge: 'start' };
      if (endX - localX <= edgeGrab) return { block, edge: 'end' };
    }
    return { block, edge: null };
  }
  for (const block of blocks) {
    if (block.loop) continue;
    const startX = viewContext.msToX(block.startMs);
    const endX = viewContext.msToX(block.endMs);
    if (Math.abs(localX - startX) <= EDGE_GRAB_PX) return { block, edge: 'start' };
    if (Math.abs(localX - endX) <= EDGE_GRAB_PX) return { block, edge: 'end' };
  }
  return null;
}

export function nudgeItem(row: RowLayout, span: NudgeSpan, deck: string): SceneItem {
  return {
    bounds: (viewContext) => ({
      x: viewContext.msToX(span.startMs),
      y: row.top,
      w: viewContext.msToX(span.endMs) - viewContext.msToX(span.startMs),
      h: row.waveformHeight
    }),
    draw: (ctx, viewContext) =>
      drawNudgeSpans(ctx, [span], row.top, row.waveformHeight, viewContext.msToX),
    hitTest: (point, viewContext) => {
      const startX = viewContext.msToX(span.startMs);
      const endX = viewContext.msToX(span.endMs);
      if (
        point.x < startX ||
        point.x > endX ||
        point.y < row.top ||
        point.y > row.top + row.waveformHeight
      )
        return null;
      return { target: 'nudgeSpan', deck, data: span };
    }
  };
}

export function laneSurfaceItem(
  lane: SublaneLayout,
  deck: string,
  deckLanes: DeckLanes | undefined
): SceneItem {
  return {
    bounds: (viewContext) => trackRect(viewContext, lane.top, lane.height),
    draw: (ctx, viewContext) => {
      // Reuse the per-lane drawers via drawDeckLanes with a single sublane.
      drawDeckLanes(
        ctx,
        viewContext.canvasW,
        viewContext.msToX,
        deckLanes,
        [lane],
        viewContext.view.start,
        viewContext.view.start + viewContext.view.duration,
        viewContext.mixerId
      );
    },
    hitTest: (point, viewContext) => {
      if (point.x < LABEL_W || point.x > LABEL_W + viewContext.trackW) return null;
      if (point.y < lane.top || point.y > lane.top + lane.height) return null;
      const pad = laneValuePad(lane.height);
      return {
        target: 'lane',
        deck,
        part: lane.key,
        data: { top: lane.top + pad, height: lane.height - 2 * pad }
      };
    }
  };
}

export function filterRegionItem(
  lane: SublaneLayout,
  deck: string,
  span: FilterActiveSpan
): SceneItem {
  return {
    bounds: (viewContext) => ({
      x: viewContext.msToX(span.startMs),
      y: lane.top,
      w: Math.max(1, viewContext.msToX(span.endMs) - viewContext.msToX(span.startMs)),
      h: lane.height
    }),
    draw: () => {},
    hitTest: (point, viewContext) => {
      const startX = viewContext.msToX(span.startMs);
      const endX = viewContext.msToX(span.endMs);
      if (point.y < lane.top || point.y > lane.top + lane.height) return null;
      if (point.x < startX - EDGE_GRAB_PX || point.x > endX + EDGE_GRAB_PX) return null;
      if (Math.abs(point.x - startX) <= EDGE_GRAB_PX)
        return { target: 'filterRegion', deck, part: 'start', data: span };
      if (Math.abs(point.x - endX) <= EDGE_GRAB_PX)
        return { target: 'filterRegion', deck, part: 'end', data: span };
      if (point.x >= startX && point.x <= endX)
        return { target: 'filterRegion', deck, part: 'body', data: span };
      return null;
    }
  };
}

// Bounds span the full track width (clipped vertically to the lane) so the
// outline's vertical edges aren't shaved off by the engine's per-item clip,
// matching the old withLaneClip-based highlight.
export function filterSelectionItem(
  lane: SublaneLayout,
  startMs: number,
  endMs: number
): SceneItem {
  return {
    bounds: (viewContext) => trackRect(viewContext, lane.top, lane.height),
    draw: (ctx, viewContext) => {
      const startX = viewContext.msToX(startMs);
      const endX = viewContext.msToX(endMs);
      ctx.strokeStyle = '#ffffffcc';
      ctx.lineWidth = 1.5;
      // Match the lane's inset value area so the outline frames the curve and its
      // bottom border clears the row divider (drawn on top of the lane).
      const pad = laneValuePad(lane.height);
      ctx.strokeRect(startX, lane.top + pad, Math.max(1, endX - startX), lane.height - 2 * pad);
    },
    hitTest: () => null
  };
}

export function laneSeparatorItem(lane: SublaneLayout, deck: string): SceneItem {
  const edgeY = lane.top + lane.height;
  return {
    bounds: (viewContext) => ({
      x: LABEL_W,
      y: edgeY - SEPARATOR_GRAB_PX,
      w: viewContext.trackW,
      h: SEPARATOR_GRAB_PX * 2
    }),
    draw: () => {},
    hitTest: (point) =>
      Math.abs(point.y - edgeY) <= SEPARATOR_GRAB_PX
        ? { target: 'laneSeparator', deck, data: lane.key }
        : null
  };
}

export function waveformSeparatorItem(row: RowLayout, deck: string): SceneItem {
  const edgeY = row.top + row.waveformHeight;
  return {
    bounds: (viewContext) => ({
      x: LABEL_W,
      y: edgeY - SEPARATOR_GRAB_PX,
      w: viewContext.trackW,
      h: SEPARATOR_GRAB_PX * 2
    }),
    draw: () => {},
    hitTest: (point) =>
      Math.abs(point.y - edgeY) <= SEPARATOR_GRAB_PX ? { target: 'waveformSeparator', deck } : null
  };
}

export function masterItem(
  top: number,
  height: number,
  lanes: MasterLanes,
  lane: MasterLaneKey
): SceneItem {
  const points = lane === 'xfader' ? lanes.xfader : lanes.gain;
  return {
    bounds: (viewContext) => ({ x: 0, y: top, w: viewContext.canvasW, h: height }),
    draw: (ctx, viewContext) => {
      drawMasterRowChrome(ctx, top, height, viewContext.canvasW, lane, viewContext.mixerId);
      drawMasterLane(
        ctx,
        points,
        lane,
        top,
        height,
        viewContext.canvasW,
        viewContext.msToX,
        viewContext.view.start,
        viewContext.view.start + viewContext.view.duration,
        viewContext.mixerId
      );
    },
    hitTest: (point, viewContext) => {
      if (point.y < top + 2 || point.y > top + height - 2) return null;
      // Label column opens the dropdown, track area is the lane, as on a deck row.
      if (point.x < LABEL_W) return { target: 'laneDropdown', deck: MASTER_ROW_ID };
      if (point.x > LABEL_W + viewContext.trackW) return null;
      return {
        target: 'lane',
        deck: MASTER_ROW_ID,
        part: lane,
        data: {
          top: top + MASTER_GAIN_INSET_Y,
          height: height - 2 * MASTER_GAIN_INSET_Y
        }
      };
    }
  };
}

export function rowDividersItem(rows: RowLayout[]): SceneItem {
  return {
    bounds: (viewContext) => ({ x: 0, y: 0, w: viewContext.canvasW, h: viewContext.canvasH }),
    draw: (ctx, viewContext) => drawRowDividers(ctx, rows, viewContext.canvasW),
    hitTest: () => null
  };
}

export function playheadItem(playheadMs: number, bottomY: number): SceneItem {
  return {
    bounds: (viewContext) => {
      const playheadX = viewContext.msToX(playheadMs);
      return {
        x: playheadX - PLAYHEAD_HALF_W,
        y: viewContext.scrollViewport.top,
        w: PLAYHEAD_HIT_W,
        h: Math.max(0, bottomY - viewContext.scrollViewport.top)
      };
    },
    draw: (ctx, viewContext) => {
      const viewEnd = viewContext.view.start + viewContext.view.duration;
      if (
        playheadMs > 0 &&
        overlapsRange(playheadMs, playheadMs, viewContext.view.start, viewEnd)
      ) {
        drawPlayhead(ctx, viewContext.msToX(playheadMs), bottomY);
      }
    },
    hitTest: () => null
  };
}

export function frameGuttersItem(): SceneItem {
  return {
    bounds: (viewContext) => ({ x: 0, y: 0, w: viewContext.canvasW, h: viewContext.canvasH }),
    draw: (ctx, viewContext) => drawFrameGutters(ctx, viewContext.canvasW, viewContext.canvasH),
    hitTest: () => null
  };
}

// The engine's `Hit.data` is `unknown` so the engine stays domain-free; this is
// where the overview's payload regains its type, beside the item that writes it.
export function readOverviewHit(hit: Hit): { part: OverviewHit; frac: number } | null {
  if (hit.target !== 'overview') return null;
  const frac = hit.data;
  if (typeof frac !== 'number' || !Number.isFinite(frac)) return null;
  const part = OVERVIEW_PARTS.find((candidate) => candidate === hit.part);
  return part ? { part, frac } : null;
}

export function overviewItem(
  totalMs: number,
  clips: Clip[],
  playheadMs: number,
  accents: Record<string, string>
): SceneItem {
  return {
    bounds: (viewContext) => ({
      x: 0,
      y: viewContext.canvasH - OVERVIEW_H,
      w: viewContext.canvasW,
      h: OVERVIEW_H
    }),
    draw: (ctx, viewContext) => {
      drawOverview(
        ctx,
        viewContext.canvasW,
        viewContext.trackW,
        viewContext.canvasH - OVERVIEW_H,
        totalMs,
        viewContext.view.start,
        viewContext.view.start + viewContext.view.duration,
        clips,
        playheadMs,
        accents
      );
    },
    hitTest: (point, viewContext) => {
      const frac = Math.max(0, Math.min(1, (point.x - LABEL_W) / (viewContext.trackW || 1)));
      const part = hitTestOverview(
        frac,
        viewContext.view,
        totalMs,
        EDGE_GRAB_PX / (viewContext.trackW || 1)
      );
      return { target: 'overview', part, data: frac };
    }
  };
}
