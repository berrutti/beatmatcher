// SceneItem factories: the reusable "components" the timeline composes. Each
// wraps an existing draw primitive (timelineDraw.ts) and adds a hitTest that
// reports a semantic Hit (target + part). Drawing is bounded to the item's rect
// by the engine, so nothing here guards its own edges. Items hold no gesture
// state; what to do with a hit is the gesture/controller layer's job.

import type { SceneItem, Rect, ViewContext } from '@renderer/utils/timelineEngine';
import type { RowLayout, SublaneLayout } from '@renderer/utils/timelineDraw';
import {
  LABEL_W,
  TICK_H,
  OVERVIEW_H,
  drawTickRow,
  drawDeckRowChrome,
  drawMasterRowChrome,
  drawMasterGainLane,
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
  type DeckRowChrome
} from '@renderer/utils/timelineDraw';
import { overlapsRange, hitTestOverview } from '@renderer/utils/timelineView';
import { blocksForDeck } from '@renderer/utils/clipEditOps';
import type { TransportBlock } from '@renderer/utils/types';
import type {
  Clip,
  LoadedSpan,
  DeckLanes,
  MasterLanes,
  NudgeSpan,
  FilterActiveSpan
} from '@renderer/composables/useSessionTimeline';
import type { TrackWaveform } from '@renderer/utils/timelineDraw';

// Thin grab tolerance for edges/separators, in pixels.
const EDGE_GRAB_PX = 6;
const SEPARATOR_GRAB_PX = 3;

// The playhead's drawn line is 1px; its bounds are widened so the clip rect
// the engine sets never clips the line at the view edges.
const PLAYHEAD_HIT_W = 3;
const PLAYHEAD_HALF_W = 1;

const trackRect = (vc: ViewContext, top: number, height: number): Rect => ({
  x: LABEL_W,
  y: top,
  w: vc.trackW,
  h: height
});

// ── tick ruler ──────────────────────────────────────────────────────────────
export function tickRowItem(): SceneItem {
  return {
    bounds: (vc) => ({ x: 0, y: 0, w: vc.canvasW, h: TICK_H }),
    draw: (ctx, vc) => drawTickRow(ctx, vc.canvasW, vc.trackW, vc.view, vc.msToX),
    hitTest: () => null
  };
}

// ── deck row background + labels + the lane dropdown caret ───────────────────
export function deckChromeItem(
  row: RowLayout,
  zebraIndex: number,
  chrome: Omit<DeckRowChrome, 'zebraIndex'>
): SceneItem {
  return {
    bounds: (vc) => ({ x: 0, y: row.top, w: vc.canvasW, h: row.height }),
    draw: (ctx, vc) => drawDeckRowChrome(ctx, row, vc.canvasW, { zebraIndex, ...chrome }),
    hitTest: (pt) => {
      // Only the label-column lane caret is interactive here.
      if (pt.x > LABEL_W || row.lanes.length === 0) return null;
      const lane = row.lanes[0];
      if (pt.y < lane.top || pt.y > lane.top + lane.height) return null;
      return { target: 'laneDropdown', deck: row.deckId };
    }
  };
}

// ── the deck's clip band (clips + loaded spans); hit-tests blocks ────────────
export function clipBandItem(
  row: RowLayout,
  clips: Clip[],
  loadedSpans: LoadedSpan[],
  waveforms: Map<string, TrackWaveform>,
  accent: string,
  audible: boolean,
  selectionSpan: { startMs: number; endMs: number } | null
): SceneItem {
  return {
    bounds: (vc) => ({ x: LABEL_W, y: row.top, w: vc.trackW, h: row.waveformHeight }),
    draw: (ctx, vc) => {
      const waveformH = row.waveformHeight;
      const viewEnd = vc.view.start + vc.view.duration;
      const visible = (s: number, e: number) => overlapsRange(s, e, vc.view.start, viewEnd);
      for (const span of loadedSpans) {
        if (span.deck === row.deckId && visible(span.startMs, span.endMs)) {
          drawLoadedSpan(ctx, span, row.top, waveformH, accent, vc.msToX);
        }
      }
      for (const clip of clips) {
        if (clip.deck === row.deckId && visible(clip.sessionStartMs, clip.sessionEndMs)) {
          drawClip(ctx, clip, waveforms.get(clip.trackPath), row.top, waveformH, accent, vc.msToX);
        }
      }
      if (!audible) {
        ctx.fillStyle = '#00000090';
        ctx.fillRect(LABEL_W, row.top, vc.trackW, waveformH);
      }
      // Track-name labels last so they stay legible over the waveform (and over
      // the inaudible dim).
      for (const span of loadedSpans) {
        if (span.deck === row.deckId && visible(span.startMs, span.endMs)) {
          drawLoadedSpanLabel(ctx, span, row.top, waveformH, vc.msToX);
        }
      }
      // Per-region BPM numbers, above the track name so each pitched/nudged
      // region reads its tempo.
      for (const clip of clips) {
        if (clip.deck === row.deckId && visible(clip.sessionStartMs, clip.sessionEndMs)) {
          drawClipBpmLabels(ctx, clip, row.top, waveformH, vc.msToX);
        }
      }
      if (selectionSpan && visible(selectionSpan.startMs, selectionSpan.endMs)) {
        drawClipSelection(
          ctx,
          selectionSpan.startMs,
          selectionSpan.endMs,
          row.top,
          waveformH,
          vc.msToX
        );
      }
    },
    hitTest: (pt, vc) => {
      if (pt.y < row.top || pt.y > row.top + row.waveformHeight || pt.x < LABEL_W) return null;
      const block = blockAtPoint(clips, row.deckId, pt.x, vc);
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

// Replicates the component's blockAtPoint: a transport block under x, plus a
// trim edge when within EDGE_GRAB_PX (loop blocks expose no edges, they move whole).
export function blockAtPoint(
  clips: Clip[],
  deck: string,
  x: number,
  vc: ViewContext
): { block: TransportBlock; edge: 'start' | 'end' | null } | null {
  for (const block of blocksForDeck(clips, deck)) {
    const x0 = vc.msToX(block.startMs);
    const x1 = vc.msToX(block.endMs);
    if (x < x0 - EDGE_GRAB_PX || x > x1 + EDGE_GRAB_PX) continue;
    if (!block.loop) {
      if (Math.abs(x - x0) <= EDGE_GRAB_PX) return { block, edge: 'start' };
      if (Math.abs(x - x1) <= EDGE_GRAB_PX) return { block, edge: 'end' };
    }
    if (x >= x0 && x <= x1) return { block, edge: null };
  }
  return null;
}

// ── nudge marker (one per span); hit covers the band so it can be grabbed ────
export function nudgeItem(row: RowLayout, span: NudgeSpan, deck: string): SceneItem {
  return {
    bounds: (vc) => ({
      x: vc.msToX(span.startMs),
      y: row.top,
      w: vc.msToX(span.endMs) - vc.msToX(span.startMs),
      h: row.waveformHeight
    }),
    draw: (ctx, vc) => drawNudgeSpans(ctx, [span], row.top, row.waveformHeight, vc.msToX),
    hitTest: (pt, vc) => {
      const x0 = vc.msToX(span.startMs);
      const x1 = vc.msToX(span.endMs);
      if (pt.x < x0 || pt.x > x1 || pt.y < row.top || pt.y > row.top + row.waveformHeight)
        return null;
      return { target: 'nudgeSpan', deck, data: span };
    }
  };
}

// ── automation lane surface (the value curve); hit = draw/paint surface ──────
export function laneSurfaceItem(
  lane: SublaneLayout,
  deck: string,
  deckLanes: DeckLanes | undefined
): SceneItem {
  return {
    bounds: (vc) => trackRect(vc, lane.top, lane.height),
    draw: (ctx, vc) => {
      // Reuse the per-lane drawers via drawDeckLanes with a single sublane.
      drawDeckLanes(
        ctx,
        vc.canvasW,
        vc.msToX,
        deckLanes,
        [lane],
        vc.view.start,
        vc.view.start + vc.view.duration
      );
    },
    hitTest: (pt, vc) => {
      if (pt.x < LABEL_W || pt.x > LABEL_W + vc.trackW) return null;
      if (pt.y < lane.top || pt.y > lane.top + lane.height) return null;
      return { target: 'lane', deck, part: lane.key, data: { top: lane.top, height: lane.height } };
    }
  };
}

// ── filter-active region: hit-only (the fill is drawn by the filter lane) ────
export function filterRegionItem(
  lane: SublaneLayout,
  deck: string,
  span: FilterActiveSpan
): SceneItem {
  return {
    bounds: (vc) => ({
      x: vc.msToX(span.startMs),
      y: lane.top,
      w: Math.max(1, vc.msToX(span.endMs) - vc.msToX(span.startMs)),
      h: lane.height
    }),
    draw: () => {},
    hitTest: (pt, vc) => {
      const x0 = vc.msToX(span.startMs);
      const x1 = vc.msToX(span.endMs);
      if (pt.y < lane.top || pt.y > lane.top + lane.height) return null;
      if (pt.x < x0 - EDGE_GRAB_PX || pt.x > x1 + EDGE_GRAB_PX) return null;
      if (Math.abs(pt.x - x0) <= EDGE_GRAB_PX)
        return { target: 'filterRegion', deck, part: 'start', data: span };
      if (Math.abs(pt.x - x1) <= EDGE_GRAB_PX)
        return { target: 'filterRegion', deck, part: 'end', data: span };
      if (pt.x >= x0 && pt.x <= x1)
        return { target: 'filterRegion', deck, part: 'body', data: span };
      return null;
    }
  };
}

// ── filter-span selection outline (white box on the filter lane) ─────────────
// Bounds span the full track width (clipped vertically to the lane) so the
// outline's vertical edges aren't shaved off by the engine's per-item clip,
// matching the old withLaneClip-based highlight.
export function filterSelectionItem(
  lane: SublaneLayout,
  startMs: number,
  endMs: number
): SceneItem {
  return {
    bounds: (vc) => trackRect(vc, lane.top, lane.height),
    draw: (ctx, vc) => {
      const x0 = vc.msToX(startMs);
      const x1 = vc.msToX(endMs);
      ctx.strokeStyle = '#ffffffcc';
      ctx.lineWidth = 1.5;
      // Match the lane's inset value area so the outline frames the curve and its
      // bottom border clears the row divider (drawn on top of the lane).
      const pad = laneValuePad(lane.height);
      ctx.strokeRect(x0, lane.top + pad, Math.max(1, x1 - x0), lane.height - 2 * pad);
    },
    hitTest: () => null
  };
}

// ── lane separator: hit-only grab band at the lane's bottom edge ─────────────
export function laneSeparatorItem(lane: SublaneLayout, deck: string): SceneItem {
  const edgeY = lane.top + lane.height;
  return {
    bounds: (vc) => ({
      x: LABEL_W,
      y: edgeY - SEPARATOR_GRAB_PX,
      w: vc.trackW,
      h: SEPARATOR_GRAB_PX * 2
    }),
    draw: () => {},
    hitTest: (pt) =>
      Math.abs(pt.y - edgeY) <= SEPARATOR_GRAB_PX
        ? { target: 'laneSeparator', deck, data: lane.key }
        : null
  };
}

// ── waveform separator: hit-only grab band at the waveform/lane boundary ──────
export function waveformSeparatorItem(row: RowLayout, deck: string): SceneItem {
  const edgeY = row.top + row.waveformHeight;
  return {
    bounds: (vc) => ({
      x: LABEL_W,
      y: edgeY - SEPARATOR_GRAB_PX,
      w: vc.trackW,
      h: SEPARATOR_GRAB_PX * 2
    }),
    draw: () => {},
    hitTest: (pt) =>
      Math.abs(pt.y - edgeY) <= SEPARATOR_GRAB_PX ? { target: 'waveformSeparator', deck } : null
  };
}

// ── master gain lane ─────────────────────────────────────────────────────────
export function masterItem(top: number, height: number, gain: MasterLanes): SceneItem {
  return {
    bounds: (vc) => ({ x: 0, y: top, w: vc.canvasW, h: height }),
    draw: (ctx, vc) => {
      drawMasterRowChrome(ctx, top, height, vc.canvasW);
      drawMasterGainLane(
        ctx,
        gain.gain,
        top,
        height,
        vc.canvasW,
        vc.msToX,
        vc.view.start,
        vc.view.start + vc.view.duration
      );
    },
    hitTest: (pt, vc) => {
      if (pt.x < LABEL_W || pt.x > LABEL_W + vc.trackW) return null;
      if (pt.y < top + 2 || pt.y > top + height - 2) return null;
      return { target: 'master', deck: 'master', part: 'masterGain' };
    }
  };
}

// ── row dividers (draw-only) ─────────────────────────────────────────────────
export function rowDividersItem(rows: RowLayout[]): SceneItem {
  return {
    bounds: (vc) => ({ x: 0, y: 0, w: vc.canvasW, h: vc.canvasH }),
    draw: (ctx, vc) => drawRowDividers(ctx, rows, vc.canvasW),
    hitTest: () => null
  };
}

// ── playhead (draw-only) ─────────────────────────────────────────────────────
export function playheadItem(playheadMs: number, bottomY: number): SceneItem {
  return {
    bounds: (vc) => {
      const playheadX = vc.msToX(playheadMs);
      return {
        x: playheadX - PLAYHEAD_HALF_W,
        y: vc.scrollViewport.top,
        w: PLAYHEAD_HIT_W,
        h: Math.max(0, bottomY - vc.scrollViewport.top)
      };
    },
    draw: (ctx, vc) => {
      const viewEnd = vc.view.start + vc.view.duration;
      if (playheadMs > 0 && overlapsRange(playheadMs, playheadMs, vc.view.start, viewEnd)) {
        drawPlayhead(ctx, vc.msToX(playheadMs), bottomY);
      }
    },
    hitTest: () => null
  };
}

// ── frame gutters (1px lines framing the track area; draw-only, drawn last) ──
export function frameGuttersItem(): SceneItem {
  return {
    bounds: (vc) => ({ x: 0, y: 0, w: vc.canvasW, h: vc.canvasH }),
    draw: (ctx, vc) => drawFrameGutters(ctx, vc.canvasW, vc.canvasH),
    hitTest: () => null
  };
}

// ── overview minimap (bottom, fixed) ─────────────────────────────────────────
export function overviewItem(
  totalMs: number,
  clips: Clip[],
  playheadMs: number,
  accents: Record<string, string>
): SceneItem {
  return {
    bounds: (vc) => ({ x: 0, y: vc.canvasH - OVERVIEW_H, w: vc.canvasW, h: OVERVIEW_H }),
    draw: (ctx, vc) => {
      drawOverview(
        ctx,
        vc.canvasW,
        vc.trackW,
        vc.canvasH - OVERVIEW_H,
        totalMs,
        vc.view.start,
        vc.view.start + vc.view.duration,
        clips,
        playheadMs,
        accents
      );
    },
    hitTest: (pt, vc) => {
      const frac = Math.max(0, Math.min(1, (pt.x - LABEL_W) / (vc.trackW || 1)));
      const part = hitTestOverview(frac, vc.view, totalMs, EDGE_GRAB_PX / (vc.trackW || 1));
      return { target: 'overview', part, data: frac };
    }
  };
}
