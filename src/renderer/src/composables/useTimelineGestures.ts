// The interaction layer. On pointer-down it hit-tests the scene (engine +
// precedence table), picks the gesture for that hit + modifiers, and drives the
// drag, emitting semantic Intents (the controller reacts). In-progress visuals
// (draw line, clip ghost, nudge/filter previews, filter-resize box) are exposed
// as overlay SceneItems so the renderer draws them like everything else.
//
// All the per-gesture behaviour ported from the old monolith lives here, but as
// small, named pieces keyed off the hit target instead of one giant switch.

import type { SceneItem, ViewContext, Hit, Point } from '@renderer/utils/timelineEngine';
import { hitScene } from '@renderer/utils/timelineEngine';
import { hitPriority } from '@renderer/utils/timelineHits';
import { LABEL_W } from '@renderer/utils/timelineDraw';
import {
  drawValueGesturePreview,
  drawNudgeGesturePreview,
  drawPaintGesturePreview,
  drawClipGhosts
} from '@renderer/utils/timelineDraw';
import { yToValue, makeMsToX, laneValuePad } from '@renderer/utils/timelineDraw';
import {
  clampFrac,
  recenterOn,
  resizeLeftEdge,
  resizeRightEdge
} from '@renderer/utils/timelineView';
import { ghostSpan, clipGestureDeltaSec } from '@renderer/utils/timelineLayout';
import {
  laneSpecFor,
  formatLaneValue,
  filterActiveAt,
  type EditableLaneKey
} from '@renderer/utils/sessionEditOps';
import { blockBounds, MIN_BLOCK_MS } from '@renderer/utils/clipEditOps';
import type { TransportBlock } from '@renderer/utils/types';
import type {
  Clip,
  LanePoint,
  DeckLanes,
  FilterActiveSpan,
  NudgeSpan
} from '@renderer/composables/useSessionTimeline';
import type { IntentHandler } from '@renderer/utils/timelineIntents';
import type { useTimelineView } from '@renderer/composables/useTimelineView';
import type { SessionEvent } from '@renderer/stores/session';

const MIN_VIEW_MS = 200;

const DRAG_THRESHOLD_PX = 3;
const EDGE_SNAP_PX = 8;

type Camera = ReturnType<typeof useTimelineView>;

export type GestureDeps = {
  camera: Camera;
  emit: IntentHandler;
  getItems: () => SceneItem[];
  getVc: () => ViewContext;
  getClips: () => Clip[];
  getEvents: () => SessionEvent[];
  getDeckLanes: () => Record<string, DeckLanes>;
  laneHeight: () => number;
  waveformHeight: () => number;
  isEditMode: () => boolean;
  durationMs: () => number;
  nudgeDirectionAt: (deck: string, y: number, rowTop: number) => 1 | -1;
  nudgeSensitivity: () => number;
  accentFor: (deck: string) => string;
  requestRender: () => void;
  setCursor: (cursor: string) => void;
};

// The drag in progress. Each variant carries just what its move/up needs.
type ActiveGesture =
  | { kind: 'track-pan'; startView: { start: number; duration: number } }
  | { kind: 'scroll-y'; startY: number; startScroll: number; trackTravel: number }
  | { kind: 'lane-resize'; startY: number; startHeight: number; height: number }
  | { kind: 'waveform-resize'; startY: number; startHeight: number; height: number }
  | {
      kind: 'overview';
      mode: 'move' | 'resize-left' | 'resize-right';
      startView: { start: number; duration: number };
    }
  | {
      kind: 'lane-draw';
      deck: string;
      lane: EditableLaneKey;
      top: number;
      height: number;
      min: number;
      max: number;
      samples: LanePoint[];
      pending: { ms: number; y: number } | null;
    }
  | {
      kind: 'nudge-paint';
      deck: string;
      rowTop: number;
      direction: 1 | -1;
      startMs: number;
      currentMs: number;
    }
  | {
      kind: 'filter-paint';
      deck: string;
      top: number;
      height: number;
      startMs: number;
      currentMs: number;
    }
  | {
      kind: 'clip';
      block: TransportBlock;
      clips: Clip[];
      rowTop: number;
      edge: 'start' | 'end' | null;
      grabMs: number;
      deltaMs: number;
      targetMs: number;
      minStartMs: number;
      maxEndMs: number;
      snapMs: number;
    }
  | {
      kind: 'filter-resize';
      deck: string;
      span: FilterActiveSpan;
      edge: 'start' | 'end';
      currentMs: number;
    }
  | {
      kind: 'filter-move';
      deck: string;
      span: FilterActiveSpan;
      grabMs: number;
      deltaMs: number;
    };

export function useTimelineGestures(deps: GestureDeps) {
  let active: ActiveGesture | null = null;
  let startClientX = 0;
  let dragged = false;

  const fracAtClientLocalX = (x: number, vc: ViewContext) =>
    clampFrac((x - LABEL_W) / (vc.trackW || 1));

  function pointFrom(e: MouseEvent, rect: DOMRect): Point {
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  }

  function hitAt(pt: Point): Hit | null {
    return hitScene(deps.getItems(), pt, deps.getVc(), hitPriority);
  }

  function laneRange(lane: EditableLaneKey, deck: string): { min: number; max: number } {
    const d = deps.getDeckLanes()[deck];
    const spec = laneSpecFor(lane, { rateMin: d?.rateMin, rateMax: d?.rateMax });
    return { min: spec.min, max: spec.max };
  }

  // ── pointer down: choose and arm a gesture, or emit a click-time intent ────
  function onMouseDown(e: MouseEvent, rect: DOMRect): void {
    const vc = deps.getVc();
    if (vc.trackW <= 0) return;
    const pt = pointFrom(e, rect);
    const hit = hitAt(pt);
    startClientX = e.clientX;
    dragged = false;
    active = null;
    if (!hit) return;

    switch (hit.target) {
      case 'scrollbar': {
        const data = hit.data as { trackH: number; thumbH: number; top: number };
        const range = data.trackH - data.thumbH;
        const max = deps.camera.maxScrollY();
        let scroll = deps.camera.scrollY.value;
        if (hit.part === 'track') {
          const targetThumbY = Math.min(
            data.top + range,
            Math.max(data.top, pt.y - data.thumbH / 2)
          );
          scroll = range > 0 ? ((targetThumbY - data.top) / range) * max : 0;
          deps.emit({ type: 'scroll.set', scrollY: scroll });
        }
        active = { kind: 'scroll-y', startY: pt.y, startScroll: scroll, trackTravel: range };
        return;
      }
      case 'overview': {
        const part = hit.part as 'move' | 'resize-left' | 'resize-right' | 'outside';
        if (part === 'outside') {
          // Recenter immediately, then drag as move.
          const frac = hit.data as number;
          const total = deps.durationMs() || 1;
          deps.emit({
            type: 'view.set',
            view: recenterOn(deps.camera.currentView(), frac * total, total, MIN_VIEW_MS)
          });
          active = { kind: 'overview', mode: 'move', startView: deps.camera.currentView() };
          return;
        }
        active = { kind: 'overview', mode: part, startView: deps.camera.currentView() };
        return;
      }
      case 'laneSeparator': {
        const h = deps.laneHeight();
        active = { kind: 'lane-resize', startY: pt.y, startHeight: h, height: h };
        return;
      }
      case 'waveformSeparator': {
        const h = deps.waveformHeight();
        active = { kind: 'waveform-resize', startY: pt.y, startHeight: h, height: h };
        return;
      }
      case 'laneDropdown':
        deps.emit({
          type: 'lane.openDropdown',
          deck: hit.deck!,
          clientX: e.clientX,
          clientY: e.clientY
        });
        return;
      case 'filterRegion': {
        // Editable while playing, like clips; the commit stops playback on drop.
        const span = hit.data as FilterActiveSpan;
        if (hit.part === 'start' || hit.part === 'end') {
          active = {
            kind: 'filter-resize',
            deck: hit.deck!,
            span,
            edge: hit.part,
            currentMs: vc.xToMs(pt.x)
          };
        } else {
          active = {
            kind: 'filter-move',
            deck: hit.deck!,
            span,
            grabMs: vc.xToMs(pt.x),
            deltaMs: 0
          };
        }
        deps.emit({ type: 'filterRegion.select', deck: hit.deck!, span });
        return; // a plain body click (no drag) just leaves it selected
      }
      case 'clip': {
        if (!deps.isEditMode()) break;
        const { block, rowTop } = hit.data as { block: TransportBlock; rowTop: number };
        // Shift+drag paints a nudge over the deck row, even on top of a clip
        // (otherwise a loaded waveform would block the gesture entirely).
        if (e.shiftKey) {
          armNudge(hit.deck!, rowTop, vc, pt);
          return;
        }
        const edge = hit.part === 'start' || hit.part === 'end' ? hit.part : null;
        armClip(block, rowTop, edge, vc, pt);
        return;
      }
      case 'lane': {
        if (!deps.isEditMode()) break;
        const ms = vc.xToMs(pt.x);
        const laneKey = hit.part as EditableLaneKey;
        const laneRect = hit.data as { top: number; height: number };
        // Match the renderer's inset value area so drawn points and the painted
        // region land where they're rendered.
        const pad = laneValuePad(laneRect.height);
        const valueTop = laneRect.top + pad;
        const valueH = laneRect.height - 2 * pad;
        if (e.shiftKey && laneKey === 'filter') {
          active = {
            kind: 'filter-paint',
            deck: hit.deck!,
            top: valueTop,
            height: valueH,
            startMs: ms,
            currentMs: ms
          };
          return;
        }
        const { min, max } = laneRange(laneKey, hit.deck!);
        // Defer starting the draw until an actual drag, so a plain click seeks.
        active = {
          kind: 'lane-draw',
          deck: hit.deck!,
          lane: laneKey,
          top: valueTop,
          height: valueH,
          min,
          max,
          samples: [],
          pending: { ms, y: pt.y }
        };
        return;
      }
      case 'clipBand': {
        if (deps.isEditMode() && e.shiftKey) {
          armNudge(hit.deck!, (hit.data as { rowTop: number }).rowTop, vc, pt);
          return;
        }
        break;
      }
    }
    // Anything not handled above falls through to a view pan (drag empty space).
    active = { kind: 'track-pan', startView: deps.camera.currentView() };
  }

  function onMouseMove(e: MouseEvent, rect: DOMRect): void {
    if (!active) return;
    const vc = deps.getVc();
    const pt = pointFrom(e, rect);
    if (Math.abs(e.clientX - startClientX) > DRAG_THRESHOLD_PX) dragged = true;

    switch (active.kind) {
      case 'track-pan':
        deps.camera.panByPixels(e.clientX - startClientX, vc.trackW);
        return;
      case 'scroll-y': {
        const max = deps.camera.maxScrollY();
        const dScroll =
          active.trackTravel > 0 ? ((pt.y - active.startY) * max) / active.trackTravel : 0;
        deps.emit({
          type: 'scroll.set',
          scrollY: Math.min(max, Math.max(0, active.startScroll + dScroll))
        });
        return;
      }
      case 'lane-resize': {
        active.height = clampLaneHeight(active.startHeight + (pt.y - active.startY));
        deps.emit({ type: 'lane.resize', height: active.height });
        return;
      }
      case 'waveform-resize': {
        active.height = clampWaveformHeight(active.startHeight + (pt.y - active.startY));
        deps.emit({ type: 'waveform.resize', height: active.height });
        return;
      }
      case 'overview': {
        const frac = fracAtClientLocalX(pt.x, vc);
        deps.emit({ type: 'view.set', view: overviewDrag(active, frac, deps.durationMs()) });
        return;
      }
      case 'lane-draw': {
        if (active.pending && dragged) {
          active.samples.push({
            ms: active.pending.ms,
            value: yToValue(active.top, active.height, active.min, active.max, active.pending.y)
          });
          active.pending = null;
        }
        if (active.pending) return;
        const ms = clampMs(vc.xToMs(pt.x), deps.durationMs());
        const last = active.samples[active.samples.length - 1];
        const value =
          e.shiftKey && last
            ? last.value
            : yToValue(active.top, active.height, active.min, active.max, pt.y);
        active.samples.push({ ms, value });
        deps.requestRender();
        return;
      }
      case 'nudge-paint':
        active.currentMs = clampMs(vc.xToMs(pt.x), deps.durationMs());
        deps.requestRender();
        return;
      case 'filter-paint':
        active.currentMs = clampMs(vc.xToMs(pt.x), deps.durationMs());
        deps.requestRender();
        return;
      case 'filter-resize': {
        active.currentMs = clampMs(vc.xToMs(pt.x), deps.durationMs());
        deps.emit({
          type: 'filterRegion.select',
          deck: active.deck,
          span: {
            startMs: active.edge === 'start' ? active.currentMs : active.span.startMs,
            endMs: active.edge === 'end' ? active.currentMs : active.span.endMs
          }
        });
        deps.requestRender();
        return;
      }
      case 'filter-move': {
        active.deltaMs = vc.xToMs(pt.x) - active.grabMs;
        deps.emit({
          type: 'filterRegion.select',
          deck: active.deck,
          span: {
            startMs: active.span.startMs + active.deltaMs,
            endMs: active.span.endMs + active.deltaMs
          }
        });
        deps.requestRender();
        return;
      }
      case 'clip':
        updateClip(active, clampMs(vc.xToMs(pt.x), deps.durationMs()));
        deps.requestRender();
        return;
    }
  }

  function onMouseUp(): void {
    const g = active;
    active = null;
    if (!g) return;
    switch (g.kind) {
      case 'lane-resize':
        deps.emit({ type: 'lane.resize', height: g.height });
        break;
      case 'waveform-resize':
        deps.emit({ type: 'waveform.resize', height: g.height });
        break;
      case 'lane-draw':
        if (g.samples.length > 0) {
          let t0 = Infinity;
          let t1 = -Infinity;
          for (const s of g.samples) {
            t0 = Math.min(t0, s.ms);
            t1 = Math.max(t1, s.ms);
          }
          deps.emit({
            type: 'lane.draw',
            deck: g.deck,
            lane: g.lane,
            samples: g.samples,
            t0,
            t1,
            rateMin: g.min,
            rateMax: g.max
          });
          dragged = true;
        }
        break;
      case 'nudge-paint':
        deps.emit({
          type: 'nudge.paint',
          deck: g.deck,
          t0: Math.min(g.startMs, g.currentMs),
          t1: Math.max(g.startMs, g.currentMs),
          direction: g.direction
        });
        dragged = true;
        break;
      case 'filter-paint':
        deps.emit({
          type: 'filter.toggle',
          deck: g.deck,
          t0: Math.min(g.startMs, g.currentMs),
          t1: Math.max(g.startMs, g.currentMs)
        });
        dragged = true;
        break;
      case 'filter-resize':
        deps.emit({
          type: 'filterRegion.resize',
          deck: g.deck,
          span: g.span,
          edge: g.edge,
          newMs: g.currentMs
        });
        dragged = true;
        break;
      case 'filter-move':
        if (!dragged) break; // a plain body click just selects (handled on click)
        if (Math.abs(g.deltaMs) >= 1)
          deps.emit({ type: 'filterRegion.move', deck: g.deck, span: g.span, deltaMs: g.deltaMs });
        break;
      case 'clip':
        if (!dragged) break; // a press without movement is a scrub click, not an edit
        if (g.edge)
          deps.emit({ type: 'clip.trim', block: g.block, edge: g.edge, newMs: g.targetMs });
        else if (Math.abs(g.deltaMs) >= 1)
          deps.emit({ type: 'clip.move', block: g.block, deltaMs: g.deltaMs });
        break;
    }
    deps.setCursor('');
    deps.requestRender();
  }

  function onClick(e: MouseEvent, rect: DOMRect): void {
    if (dragged) {
      dragged = false;
      return;
    }
    const vc = deps.getVc();
    const pt = pointFrom(e, rect);
    const hit = hitAt(pt);
    const ms = vc.xToMs(pt.x);
    if (!hit) {
      deps.emit({ type: 'clip.clearSelection' });
      deps.emit({ type: 'filterRegion.clearSelection' });
      return;
    }
    if (hit.target === 'clip') {
      const { block } = hit.data as { block: TransportBlock };
      deps.emit({ type: 'clip.select', block, ms });
      deps.emit({ type: 'filterRegion.clearSelection' });
      deps.emit({ type: 'seek', ms });
      return;
    }
    if (hit.target === 'filterRegion') {
      deps.emit({
        type: 'filterRegion.select',
        deck: hit.deck!,
        span: hit.data as FilterActiveSpan
      });
      deps.emit({ type: 'seek', ms });
      return;
    }
    if (hit.target === 'laneDropdown' || hit.target === 'scrollbar' || hit.target === 'overview')
      return;
    // lane / clipBand / master background: seek and clear selections.
    deps.emit({ type: 'clip.clearSelection' });
    deps.emit({ type: 'filterRegion.clearSelection' });
    deps.emit({ type: 'seek', ms });
  }

  function onDblClick(e: MouseEvent, rect: DOMRect): void {
    const pt = pointFrom(e, rect);
    const hit = hitAt(pt);
    if (!hit) return;
    if (hit.target === 'laneSeparator') {
      deps.emit({ type: 'lane.resizeReset' });
      return;
    }
    if (hit.target === 'waveformSeparator') {
      deps.emit({ type: 'waveform.resizeReset' });
      return;
    }
    if (hit.target === 'clip' && deps.isEditMode()) {
      const { block } = hit.data as { block: TransportBlock };
      if (block.loop)
        deps.emit({ type: 'loopBlock.toggleUnlock', block, ms: deps.getVc().xToMs(pt.x) });
    }
  }

  function onContextMenu(e: MouseEvent, rect: DOMRect): void {
    const pt = pointFrom(e, rect);
    const hit = hitAt(pt);
    if (!hit) return;
    if (hit.target === 'filterRegion' && deps.isEditMode()) {
      deps.emit({
        type: 'menu.filterRegion',
        deck: hit.deck!,
        span: hit.data as FilterActiveSpan,
        clientX: e.clientX,
        clientY: e.clientY
      });
      return;
    }
    if (hit.deck && hit.deck !== 'master') {
      deps.emit({
        type: 'menu.deck',
        deck: hit.deck,
        clientX: e.clientX,
        clientY: e.clientY,
        nudge: hit.target === 'nudgeSpan' ? (hit.data as NudgeSpan) : null
      });
    }
  }

  function onWheel(e: WheelEvent, rect: DOMRect): void {
    const vc = deps.getVc();
    if (vc.trackW <= 0) return;
    const pt = pointFrom(e, rect);
    if (e.ctrlKey || e.metaKey) {
      deps.camera.zoomAt(fracAtClientLocalX(pt.x, vc), e.deltaY);
    } else if (deps.camera.maxScrollY() > 0 && Math.abs(e.deltaY) >= Math.abs(e.deltaX)) {
      deps.camera.scrollByPixels(e.deltaY);
      deps.requestRender();
    } else {
      deps.camera.panByMsDelta((e.deltaX || e.deltaY) * (vc.view.duration / vc.trackW));
    }
  }

  // Overlay items for the active gesture's preview, appended to the scene.
  function overlays(): SceneItem[] {
    if (!active) return [];
    const vc = deps.getVc();
    const msToX = makeMsToX(vc.view, vc.trackW);
    if (active.kind === 'lane-draw' && active.samples.length > 0) {
      const g = active;
      const cursor = g.samples[g.samples.length - 1];
      return [
        overlay((ctx) =>
          drawValueGesturePreview(
            ctx,
            g,
            normalize(g.samples),
            formatLaneValue(g.lane, cursor.value),
            cursor.ms,
            msToX,
            vc.canvasW
          )
        )
      ];
    }
    if (active.kind === 'nudge-paint') {
      const g = active;
      return [
        overlay((ctx) =>
          drawNudgeGesturePreview(
            ctx,
            Math.min(g.startMs, g.currentMs),
            Math.max(g.startMs, g.currentMs),
            g.direction * deps.nudgeSensitivity(),
            g.rowTop,
            deps.waveformHeight(),
            g.currentMs,
            msToX,
            vc.canvasW
          )
        )
      ];
    }
    if (active.kind === 'filter-paint') {
      const g = active;
      const t0 = Math.min(g.startMs, g.currentMs);
      const want = !filterActiveAt(deps.getEvents(), g.deck, t0, false);
      return [
        overlay((ctx) =>
          drawPaintGesturePreview(
            ctx,
            t0,
            Math.max(g.startMs, g.currentMs),
            want,
            g.top,
            g.height,
            g.currentMs,
            msToX,
            vc.canvasW
          )
        )
      ];
    }
    if (active.kind === 'clip' && dragged) {
      const g = active;
      const kind = g.edge ? (g.edge === 'start' ? 'trim-start' : 'trim-end') : 'move';
      const deltaSec = clipGestureDeltaSec(
        kind,
        g.deltaMs,
        g.targetMs,
        g.block.startMs,
        g.block.endMs
      );
      return [
        overlay((ctx) =>
          drawClipGhosts(
            ctx,
            g.clips.map((c) => ghostSpan(c, { kind, deltaMs: g.deltaMs, targetMs: g.targetMs })),
            g.rowTop,
            deps.waveformHeight(),
            deps.accentFor(g.block.deck),
            `${deltaSec > 0 ? '+' : ''}${deltaSec.toFixed(2)}s`,
            g.block.startMs + (kind === 'move' ? g.deltaMs : 0),
            msToX,
            vc.canvasW
          )
        )
      ];
    }
    return [];
  }

  function cursorFor(pt: Point): string {
    const hit = hitAt(pt);
    if (!hit) return '';
    if (hit.target === 'scrollbar') return 'pointer';
    if (hit.target === 'laneSeparator' || hit.target === 'waveformSeparator') return 'row-resize';
    if (hit.target === 'overview')
      return hit.part === 'resize-left' || hit.part === 'resize-right' ? 'ew-resize' : 'grab';
    if (hit.target === 'filterRegion') return hit.part === 'body' ? 'grab' : 'ew-resize';
    if (hit.target === 'clip') {
      if (!deps.isEditMode()) return '';
      return hit.part === 'start' || hit.part === 'end' ? 'ew-resize' : 'grab';
    }
    if (hit.target === 'laneDropdown') return 'pointer';
    if (hit.target === 'lane') return 'crosshair';
    return '';
  }

  // ── small helpers ──────────────────────────────────────────────────────────
  function armNudge(deck: string, rowTop: number, vc: ViewContext, pt: Point): void {
    const ms = vc.xToMs(pt.x);
    active = {
      kind: 'nudge-paint',
      deck,
      rowTop,
      direction: deps.nudgeDirectionAt(deck, pt.y, rowTop),
      startMs: ms,
      currentMs: ms
    };
  }

  function armClip(
    block: TransportBlock,
    rowTop: number,
    edge: 'start' | 'end' | null,
    vc: ViewContext,
    pt: Point
  ): void {
    const allClips = deps.getClips();
    const bounds = blockBounds(deps.getEvents(), allClips, block);
    const grabMs = vc.xToMs(pt.x);
    const blockClips = allClips.filter(
      (c) =>
        c.deck === block.deck &&
        c.sessionStartMs >= block.startMs - 1 &&
        c.sessionEndMs <= block.endMs + 1
    );
    active = {
      kind: 'clip',
      block,
      clips: blockClips,
      rowTop,
      edge,
      grabMs,
      deltaMs: 0,
      targetMs: edge === 'start' ? block.startMs : block.endMs,
      minStartMs: bounds?.minStartMs ?? 0,
      maxEndMs: bounds?.maxEndMs ?? Infinity,
      snapMs: (vc.view.duration / vc.trackW) * EDGE_SNAP_PX
    };
  }

  // Edge magnetism (NOT beat snapping): within a few pixels an edge locks onto a
  // neighbour boundary or the block's own original position, so placing a clip
  // "touching" or "back where it was" by eye is sample-exact. Mirrors the old
  // component's updateClipGesture.
  function updateClip(g: Extract<ActiveGesture, { kind: 'clip' }>, pointerMs: number): void {
    const { block, snapMs } = g;
    if (!g.edge) {
      let delta = pointerMs - g.grabMs;
      const rawStart = block.startMs + delta;
      const snappedStart = snapToEdges(rawStart, [block.startMs, g.minStartMs], snapMs);
      if (snappedStart !== rawStart) {
        delta = snappedStart - block.startMs;
      } else {
        const rawEnd = block.endMs + delta;
        const snappedEnd = snapToEdges(rawEnd, [g.maxEndMs], snapMs);
        if (snappedEnd !== rawEnd) delta = snappedEnd - block.endMs;
      }
      g.deltaMs = Math.max(g.minStartMs - block.startMs, Math.min(g.maxEndMs - block.endMs, delta));
      return;
    }
    if (g.edge === 'start') {
      const target = snapToEdges(pointerMs, [block.startMs, g.minStartMs], snapMs);
      const earliestByAudio = block.startMs - (block.trackStartSec / block.playbackRate) * 1000;
      g.targetMs = Math.max(
        Math.max(g.minStartMs, earliestByAudio),
        Math.min(block.endMs - MIN_BLOCK_MS, target)
      );
    } else {
      const target = snapToEdges(pointerMs, [block.endMs, g.maxEndMs], snapMs);
      g.targetMs = Math.max(block.startMs + MIN_BLOCK_MS, Math.min(g.maxEndMs, target));
    }
  }

  return {
    onMouseDown,
    onMouseMove,
    onMouseUp,
    onClick,
    onDblClick,
    onContextMenu,
    onWheel,
    overlays,
    cursorFor,
    isDragging: () => dragged,
    hasActive: () => active !== null
  };
}

// ── module-local pure helpers ─────────────────────────────────────────────────
function overlay(draw: (ctx: CanvasRenderingContext2D) => void): SceneItem {
  return {
    bounds: (vc) => ({ x: 0, y: 0, w: vc.canvasW, h: vc.canvasH }),
    draw: (ctx) => draw(ctx),
    hitTest: () => null
  };
}

function clampMs(ms: number, total: number): number {
  return Math.min(total || 1, Math.max(0, ms));
}

function snapToEdges(value: number, candidates: number[], toleranceMs: number): number {
  let best = value;
  let bestDistance = toleranceMs;
  for (const candidate of candidates) {
    if (!Number.isFinite(candidate)) continue;
    const distance = Math.abs(value - candidate);
    if (distance < bestDistance) {
      best = candidate;
      bestDistance = distance;
    }
  }
  return best;
}

function clampLaneHeight(h: number): number {
  return Math.min(240, Math.max(10, h));
}

function clampWaveformHeight(h: number): number {
  return Math.min(240, Math.max(40, h));
}

function overviewDrag(
  g: Extract<ActiveGesture, { kind: 'overview' }>,
  frac: number,
  total: number
): { start: number; duration: number } {
  const ms = frac * (total || 1);
  if (g.mode === 'resize-left') return resizeLeftEdge(g.startView, ms, total || 1, MIN_VIEW_MS);
  if (g.mode === 'resize-right') return resizeRightEdge(g.startView, ms, total || 1, MIN_VIEW_MS);
  return recenterOn(g.startView, ms, total || 1, MIN_VIEW_MS);
}

function normalize(samples: LanePoint[]): LanePoint[] {
  const byMs = new Map<number, number>();
  for (const s of samples) byMs.set(s.ms, s.value);
  return [...byMs.entries()].map(([ms, value]) => ({ ms, value })).sort((a, b) => a.ms - b.ms);
}
