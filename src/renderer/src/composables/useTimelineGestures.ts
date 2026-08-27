import type { SceneItem, ViewContext, Hit, Point } from '@renderer/utils/timelineEngine';
import { hitScene } from '@renderer/utils/timelineEngine';
import { hitPriority, type HitTarget } from '@renderer/utils/timelineHits';
import { LABEL_W } from '@renderer/utils/timelineDraw';
import {
  drawValueGesturePreview,
  drawPaintGesturePreview,
  drawClipGhosts
} from '@renderer/utils/timelineDraw';
import { yToValue, makeMsToX } from '@renderer/utils/timelineDraw';
import {
  clampFrac,
  panByMs,
  recenterOn,
  resizeLeftEdge,
  resizeRightEdge
} from '@renderer/utils/timelineView';
import { readOverviewHit } from '@renderer/utils/timelineItems';
import { ghostSpan, clipGestureDeltaSec, marqueeTargets } from '@renderer/utils/timelineLayout';
import { laneSpecFor, formatLaneValue } from '@renderer/utils/sessionEditOps';
import type { LaneSpec } from '@renderer/utils/sessionCore';
import {
  blockBounds,
  blocksForDeck,
  filterActiveAt,
  normalizeGestureSamples
} from '@renderer/utils/sessionCore';
import type { RowLayout } from '@renderer/utils/timelineDraw';
import type {
  EditableLaneKey,
  TransportBlock,
  Clip,
  LanePoint,
  DeckLanes,
  FilterActiveSpan
} from '@renderer/utils/types';
import { isEditableLaneKey } from '@renderer/utils/types';
import { clampLaneHeight, clampWaveformHeight } from '@renderer/utils/laneHeights';
import type { BpmContext, IntentHandler } from '@renderer/utils/timelineIntents';
import type { useTimelineView } from '@renderer/composables/useTimelineView';

import type { SessionEvent } from '@renderer/utils/types';

const MIN_VIEW_MS = 200;

const DRAG_THRESHOLD_PX = 3;
const EDGE_SNAP_PX = 8;
const LANE_MENU_GAP_PX = 6;

// The label column and the separators are click targets of their own: a
// right-click there is not a request for the deck menu.
const DECK_MENU_TARGETS: readonly HitTarget[] = ['lane', 'clip', 'clipBand', 'filterRegion'];

type Camera = ReturnType<typeof useTimelineView>;

export type GestureDeps = {
  camera: Camera;
  emit: IntentHandler;
  getItems: () => SceneItem[];
  getRows: () => RowLayout[];
  getVc: () => ViewContext;
  getClips: () => Clip[];
  getEvents: () => SessionEvent[];
  getDeckLanes: () => Record<string, DeckLanes>;
  laneHeightFor: (deck: string, lane: EditableLaneKey) => number;
  waveformHeightFor: (deck: string) => number;
  isEditMode: () => boolean;
  durationMs: () => number;
  accentFor: (deck: string) => string;
  requestRender: () => void;
  setCursor: (cursor: string) => void;
};

type ActiveGesture =
  | { kind: 'track-pan'; startView: { start: number; duration: number } }
  | {
      kind: 'lane-resize';
      deck: string;
      lane: EditableLaneKey;
      startY: number;
      startHeight: number;
      height: number;
    }
  | { kind: 'waveform-resize'; deck: string; startY: number; startHeight: number; height: number }
  | {
      kind: 'overview';
      mode: 'move' | 'resize-left' | 'resize-right';
      startView: { start: number; duration: number };
      grabFrac: number;
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
      // Refreshed per mousemove so the per-frame overlay draw stays WASM-free.
      normalized: LanePoint[];
      pending: { ms: number; y: number } | null;
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
      startTrimMinMs: number;
      minBlockMs: number;
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
    }
  | { kind: 'marquee'; additive: boolean; start: Point; current: Point };

export function useTimelineGestures(deps: GestureDeps) {
  let active: ActiveGesture | null = null;
  let startClientX = 0;
  let startClientY = 0;
  let dragged = false;

  const fracAtClientLocalX = (x: number, viewContext: ViewContext) =>
    clampFrac((x - LABEL_W) / (viewContext.trackW || 1));

  function laneAnchor(data: unknown): number | null {
    if (typeof data !== 'object' || data === null || !('top' in data)) return null;
    const { top } = data;
    return typeof top === 'number' ? top : null;
  }

  function pointFrom(event: MouseEvent, rect: DOMRect): Point {
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  function hitAt(point: Point): Hit | null {
    return hitScene(deps.getItems(), point, deps.getVc(), hitPriority);
  }

  function laneSpec(lane: EditableLaneKey, deck: string): LaneSpec {
    const deckLanes = deps.getDeckLanes()[deck];
    return laneSpecFor(lane, deps.getVc().mixerId, {
      rateMin: deckLanes?.rateMin,
      rateMax: deckLanes?.rateMax
    });
  }

  function laneRange(lane: EditableLaneKey, deck: string): { min: number; max: number } {
    const spec = laneSpec(lane, deck);
    return { min: spec.min, max: spec.max };
  }

  function onMouseDown(event: MouseEvent, rect: DOMRect): void {
    const viewContext = deps.getVc();
    if (viewContext.trackW <= 0) return;
    // The right button belongs to onContextMenu; arming here would rubber-band
    // a selection under the menu it opens.
    if (event.button !== 0) return;
    const point = pointFrom(event, rect);
    const hit = hitAt(point);
    startClientX = event.clientX;
    startClientY = event.clientY;
    dragged = false;
    active = null;
    if (!hit) return;

    switch (hit.target) {
      case 'overview': {
        const overview = readOverviewHit(hit);
        if (!overview) return;
        const { part, frac: grabFrac } = overview;
        if (part === 'outside') {
          const total = deps.durationMs() || 1;
          deps.emit({
            type: 'view.set',
            view: recenterOn(deps.camera.currentView(), grabFrac * total, total, MIN_VIEW_MS)
          });
          active = {
            kind: 'overview',
            mode: 'move',
            startView: deps.camera.currentView(),
            grabFrac
          };
          return;
        }
        active = { kind: 'overview', mode: part, startView: deps.camera.currentView(), grabFrac };
        return;
      }
      case 'laneSeparator': {
        const lane = hit.data;
        const deck = hit.deck;
        if (!deck || !isEditableLaneKey(lane)) return;
        const laneHeight = deps.laneHeightFor(deck, lane);
        active = {
          kind: 'lane-resize',
          deck,
          lane,
          startY: point.y,
          startHeight: laneHeight,
          height: laneHeight
        };
        return;
      }
      case 'waveformSeparator': {
        const deck = hit.deck;
        if (!deck) return;
        const waveformHeight = deps.waveformHeightFor(deck);
        active = {
          kind: 'waveform-resize',
          deck,
          startY: point.y,
          startHeight: waveformHeight,
          height: waveformHeight
        };
        return;
      }
      case 'deckLabel': {
        if (!hit.deck) return;
        const anchor = laneAnchor(hit.data);
        deps.emit({
          type: 'menu.deck',
          deck: hit.deck,
          clientX: rect.left + LABEL_W + LANE_MENU_GAP_PX,
          clientY: anchor === null ? event.clientY : rect.top + anchor,
          bpm: null,
          split: null,
          lane: null
        });
        return;
      }
      case 'laneDropdown': {
        if (!hit.deck) return;
        const anchor = laneAnchor(hit.data);
        deps.emit({
          type: 'lane.openDropdown',
          deck: hit.deck,
          lane: isEditableLaneKey(hit.part) ? hit.part : null,
          clientX: rect.left + LABEL_W + LANE_MENU_GAP_PX,
          clientY: anchor === null ? event.clientY : rect.top + anchor
        });
        return;
      }
      case 'filterRegion': {
        // Editable while playing, like clips. The commit stops playback on drop.
        const span = hit.data as FilterActiveSpan;
        if (hit.part === 'start' || hit.part === 'end') {
          active = {
            kind: 'filter-resize',
            deck: hit.deck!,
            span,
            edge: hit.part,
            currentMs: viewContext.xToMs(point.x)
          };
        } else {
          active = {
            kind: 'filter-move',
            deck: hit.deck!,
            span,
            grabMs: viewContext.xToMs(point.x),
            deltaMs: 0
          };
        }
        deps.emit({ type: 'filterRegion.select', deck: hit.deck!, span });
        return; // a plain body click (no drag) just leaves it selected
      }
      case 'clip': {
        if (!deps.isEditMode()) break;
        const { block, rowTop } = hit.data as { block: TransportBlock; rowTop: number };
        // Cmd/Ctrl+drag draws a marquee even over clips (a plain press would
        // grab the block). Without a drag the click toggles the block instead.
        if (event.metaKey || event.ctrlKey) {
          active = { kind: 'marquee', additive: true, start: point, current: point };
          return;
        }
        const edge = hit.part === 'start' || hit.part === 'end' ? hit.part : null;
        armClip(block, rowTop, edge, viewContext, point);
        return;
      }
      case 'lane': {
        if (!deps.isEditMode()) break;
        const ms = viewContext.xToMs(point.x);
        const laneKey = hit.part as EditableLaneKey;
        // The item reports the value area it draws into, which is inset from the
        // lane frame by a different amount on the master row than on a deck.
        const { top: valueTop, height: valueH } = hit.data as { top: number; height: number };
        if (event.shiftKey && laneKey === 'filter') {
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
          normalized: [],
          pending: { ms, y: point.y }
        };
        return;
      }
      case 'clipBand': {
        // Dragging empty band space in edit mode rubber-band selects. Panning
        // stays available via wheel, the overview, and outside edit mode.
        if (deps.isEditMode()) {
          active = {
            kind: 'marquee',
            additive: event.metaKey || event.ctrlKey,
            start: point,
            current: point
          };
          return;
        }
        break;
      }
    }
    active = { kind: 'track-pan', startView: deps.camera.currentView() };
  }

  function onMouseMove(event: MouseEvent, rect: DOMRect): void {
    if (!active) return;
    const viewContext = deps.getVc();
    const point = pointFrom(event, rect);
    // Both axes: the separators resize vertically, and an X-only test let the
    // trailing click fall through to the seek branch.
    if (
      Math.abs(event.clientX - startClientX) > DRAG_THRESHOLD_PX ||
      Math.abs(event.clientY - startClientY) > DRAG_THRESHOLD_PX
    )
      dragged = true;

    switch (active.kind) {
      case 'track-pan':
        deps.camera.panByPixels(event.clientX - startClientX, viewContext.trackW);
        return;
      case 'lane-resize': {
        active.height = clampLaneHeight(active.startHeight + (point.y - active.startY));
        deps.emit({
          type: 'lane.resize',
          deck: active.deck,
          lane: active.lane,
          height: active.height
        });
        return;
      }
      case 'waveform-resize': {
        active.height = clampWaveformHeight(active.startHeight + (point.y - active.startY));
        deps.emit({ type: 'waveform.resize', deck: active.deck, height: active.height });
        return;
      }
      case 'overview': {
        const frac = fracAtClientLocalX(point.x, viewContext);
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
        const ms = clampMs(viewContext.xToMs(point.x), deps.durationMs());
        const last = active.samples[active.samples.length - 1];
        const value =
          event.shiftKey && last
            ? last.value
            : yToValue(active.top, active.height, active.min, active.max, point.y);
        active.samples.push({ ms, value });
        active.normalized = normalizeGestureSamples(active.samples);
        deps.requestRender();
        return;
      }
      case 'filter-paint':
        active.currentMs = clampMs(viewContext.xToMs(point.x), deps.durationMs());
        deps.requestRender();
        return;
      case 'filter-resize': {
        active.currentMs = clampMs(viewContext.xToMs(point.x), deps.durationMs());
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
        active.deltaMs = viewContext.xToMs(point.x) - active.grabMs;
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
        updateClip(active, clampMs(viewContext.xToMs(point.x), deps.durationMs()));
        deps.requestRender();
        return;
      case 'marquee':
        active.current = point;
        deps.requestRender();
        return;
    }
  }

  function onMouseUp(): void {
    const gesture = active;
    active = null;
    if (!gesture) return;
    switch (gesture.kind) {
      case 'lane-resize':
        deps.emit({
          type: 'lane.resize',
          deck: gesture.deck,
          lane: gesture.lane,
          height: gesture.height
        });
        break;
      case 'waveform-resize':
        deps.emit({ type: 'waveform.resize', deck: gesture.deck, height: gesture.height });
        break;
      case 'lane-draw':
        if (gesture.samples.length > 0) {
          let minMs = Infinity;
          let maxMs = -Infinity;
          for (const sample of gesture.samples) {
            minMs = Math.min(minMs, sample.ms);
            maxMs = Math.max(maxMs, sample.ms);
          }
          deps.emit({
            type: 'lane.draw',
            deck: gesture.deck,
            lane: gesture.lane,
            samples: gesture.samples,
            t0: minMs,
            t1: maxMs,
            rateMin: gesture.min,
            rateMax: gesture.max
          });
          dragged = true;
        }
        break;
      case 'filter-paint':
        deps.emit({
          type: 'filter.toggle',
          deck: gesture.deck,
          t0: Math.min(gesture.startMs, gesture.currentMs),
          t1: Math.max(gesture.startMs, gesture.currentMs)
        });
        dragged = true;
        break;
      case 'filter-resize':
        if (!dragged) break; // a plain edge click just selects, like the body
        deps.emit({
          type: 'filterRegion.resize',
          deck: gesture.deck,
          span: gesture.span,
          edge: gesture.edge,
          newMs: gesture.currentMs
        });
        dragged = true;
        break;
      case 'filter-move':
        if (!dragged) break; // a plain body click just selects (handled on click)
        if (Math.abs(gesture.deltaMs) >= 1)
          deps.emit({
            type: 'filterRegion.move',
            deck: gesture.deck,
            span: gesture.span,
            deltaMs: gesture.deltaMs
          });
        break;
      case 'clip':
        if (!dragged) break; // a press without movement is a scrub click, not an edit
        if (gesture.edge)
          deps.emit({
            type: 'clip.trim',
            block: gesture.block,
            edge: gesture.edge,
            newMs: gesture.targetMs
          });
        else if (Math.abs(gesture.deltaMs) >= 1)
          deps.emit({ type: 'clip.move', block: gesture.block, deltaMs: gesture.deltaMs });
        break;
      case 'marquee': {
        if (!dragged) break; // a press without movement is a click (select/scrub)
        const targets = marqueeTargets(
          deps.getRows(),
          (deck) => blocksForDeck(deps.getClips(), deck),
          {
            x0: gesture.start.x,
            x1: gesture.current.x,
            y0: gesture.start.y,
            y1: gesture.current.y
          },
          deps.getVc().xToMs
        );
        deps.emit({ type: 'clip.selectRange', targets, additive: gesture.additive });
        deps.emit({ type: 'filterRegion.clearSelection' });
        break;
      }
    }
    deps.setCursor('');
    deps.requestRender();
  }

  // The span's grab bar is only its top strip, so the rest of the lane stays
  // drawable; a plain click anywhere inside it still selects it.
  function activeFilterSpanAt(
    hit: Hit,
    ms: number
  ): { deck: string; span: FilterActiveSpan } | null {
    if (hit.target !== 'lane' || hit.part !== 'filter' || !hit.deck) return null;
    const span = deps
      .getDeckLanes()
      [hit.deck]?.filterActive.find((active) => ms >= active.startMs && ms <= active.endMs);
    return span ? { deck: hit.deck, span } : null;
  }

  function onClick(event: MouseEvent, rect: DOMRect): void {
    if (dragged) {
      dragged = false;
      return;
    }
    const viewContext = deps.getVc();
    const point = pointFrom(event, rect);
    const hit = hitAt(point);
    const ms = viewContext.xToMs(point.x);
    if (!hit) {
      deps.emit({ type: 'clip.clearSelection' });
      deps.emit({ type: 'filterRegion.clearSelection' });
      return;
    }
    if (hit.target === 'clip') {
      const { block } = hit.data as { block: TransportBlock };
      const additive = deps.isEditMode() && (event.metaKey || event.ctrlKey);
      deps.emit({ type: 'clip.select', block, ms, additive });
      deps.emit({ type: 'filterRegion.clearSelection' });
      // Cmd/Ctrl-click only edits the selection. Moving the playhead too would
      // make assembling a multi-selection jumpy.
      if (!additive) deps.emit({ type: 'seek', ms });
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
    if (hit.target === 'laneDropdown' || hit.target === 'overview') return;
    const span = activeFilterSpanAt(hit, ms);
    if (span) {
      deps.emit({ type: 'clip.clearSelection' });
      deps.emit({ type: 'filterRegion.select', deck: span.deck, span: span.span });
      deps.emit({ type: 'seek', ms });
      return;
    }
    deps.emit({ type: 'clip.clearSelection' });
    deps.emit({ type: 'filterRegion.clearSelection' });
    deps.emit({ type: 'seek', ms });
  }

  function onDblClick(event: MouseEvent, rect: DOMRect): void {
    const point = pointFrom(event, rect);
    const hit = hitAt(point);
    if (!hit) return;
    if (hit.target === 'laneSeparator' && hit.deck && isEditableLaneKey(hit.data)) {
      deps.emit({ type: 'lane.resizeReset', deck: hit.deck, lane: hit.data });
      return;
    }
    if (hit.target === 'waveformSeparator' && hit.deck) {
      deps.emit({ type: 'waveform.resizeReset', deck: hit.deck });
      return;
    }
    if (hit.target === 'clip' && deps.isEditMode()) {
      const { block } = hit.data as { block: TransportBlock };
      if (block.loop) {
        deps.emit({ type: 'loopBlock.toggleUnlock', block, ms: deps.getVc().xToMs(point.x) });
        return;
      }
      // Double-click a regular block: whole block (a single click picks only
      // the BPM region under the cursor).
      deps.emit({
        type: 'clip.selectRange',
        targets: [{ deck: block.deck, startMs: block.startMs, endMs: block.endMs }],
        additive: false
      });
    }
  }

  // Null unless a pitchable clip sits under `ms`, so the menu item appears only
  // where it would work.
  function bpmContextAt(deck: string, ms: number): BpmContext | null {
    for (const clip of deps.getClips()) {
      if (clip.deck !== deck || clip.loop) continue;
      if (ms < clip.sessionStartMs || ms > clip.sessionEndMs) continue;
      // Both bounds are inclusive, so at a shared millisecond two clips both match.
      if (!clip.bpm || clip.bpm <= 0) continue;
      const seg = clip.waveSegments.find(
        (segment) => ms >= segment.wallStartMs && ms <= segment.wallEndMs
      );
      const wallSec = seg ? (seg.wallEndMs - seg.wallStartMs) / 1000 : 0;
      const trackSpan = seg ? seg.trackEndSec - seg.trackStartSec : 0;
      const effRate = seg && wallSec > 0 && trackSpan > 0 ? trackSpan / wallSec : clip.playbackRate;
      return {
        ms,
        clipStartMs: clip.sessionStartMs,
        clipEndMs: clip.sessionEndMs,
        trackBpm: clip.bpm,
        currentBpm: clip.bpm * effRate
      };
    }
    return null;
  }

  function onContextMenu(event: MouseEvent, rect: DOMRect): void {
    const point = pointFrom(event, rect);
    const hit = hitAt(point);
    if (!hit) return;
    if (hit.target === 'filterRegion' && deps.isEditMode()) {
      deps.emit({
        type: 'menu.filterRegion',
        deck: hit.deck!,
        span: hit.data as FilterActiveSpan,
        clientX: event.clientX,
        clientY: event.clientY
      });
      return;
    }
    if (hit.deck && DECK_MENU_TARGETS.some((target) => target === hit.target)) {
      const laneKey = hit.target === 'lane' ? hit.part : null;
      deps.emit({
        type: 'menu.deck',
        deck: hit.deck,
        lane:
          laneKey && isEditableLaneKey(laneKey)
            ? { key: laneKey, ms: deps.getVc().xToMs(point.x) }
            : null,
        clientX: event.clientX,
        clientY: event.clientY,
        bpm: hit.target === 'clip' ? bpmContextAt(hit.deck, deps.getVc().xToMs(point.x)) : null,
        split:
          hit.target === 'clip'
            ? {
                block: (hit.data as { block: TransportBlock }).block,
                ms: deps.getVc().xToMs(point.x)
              }
            : null
      });
    }
  }

  function onWheel(event: WheelEvent, rect: DOMRect): void {
    const viewContext = deps.getVc();
    if (viewContext.trackW <= 0) return;
    const point = pointFrom(event, rect);
    if (event.ctrlKey || event.metaKey) {
      event.preventDefault();
      deps.camera.zoomAt(fracAtClientLocalX(point.x, viewContext), event.deltaY);
    } else if (deps.camera.maxScrollY() > 0 && Math.abs(event.deltaY) >= Math.abs(event.deltaX)) {
      // Vertical scroll is owned by the native scroll container. Don't
      // preventDefault so the browser scrolls it and fires its scroll event.
    } else {
      event.preventDefault();
      deps.camera.panByMsDelta(
        (event.deltaX || event.deltaY) * (viewContext.view.duration / viewContext.trackW)
      );
    }
  }

  // Overlay items for the active gesture's preview, appended to the scene.
  function overlays(): SceneItem[] {
    if (!active) return [];
    const viewContext = deps.getVc();
    const msToX = makeMsToX(viewContext.view, viewContext.trackW);
    if (active.kind === 'lane-draw' && active.samples.length > 0) {
      const gesture = active;
      const cursor = gesture.samples[gesture.samples.length - 1];
      return [
        overlay((ctx) =>
          drawValueGesturePreview(
            ctx,
            gesture,
            gesture.normalized,
            formatLaneValue(laneSpec(gesture.lane, gesture.deck), cursor.value),
            cursor.ms,
            msToX,
            viewContext.canvasW
          )
        )
      ];
    }
    if (active.kind === 'filter-paint') {
      const gesture = active;
      const startMs = Math.min(gesture.startMs, gesture.currentMs);
      const wantActive = !filterActiveAt(deps.getEvents(), gesture.deck, startMs, false);
      return [
        overlay((ctx) =>
          drawPaintGesturePreview(
            ctx,
            startMs,
            Math.max(gesture.startMs, gesture.currentMs),
            wantActive,
            gesture.top,
            gesture.height,
            gesture.currentMs,
            msToX,
            viewContext.canvasW
          )
        )
      ];
    }
    if (active.kind === 'marquee' && dragged) {
      const gesture = active;
      return [
        overlay((ctx) => {
          const x = Math.min(gesture.start.x, gesture.current.x);
          const y = Math.min(gesture.start.y, gesture.current.y);
          const width = Math.abs(gesture.current.x - gesture.start.x);
          const height = Math.abs(gesture.current.y - gesture.start.y);
          ctx.fillStyle = '#ffffff14';
          ctx.fillRect(x, y, width, height);
          ctx.strokeStyle = '#ffffffcc';
          ctx.lineWidth = 1;
          ctx.strokeRect(x, y, width, height);
        })
      ];
    }
    if (active.kind === 'clip' && dragged) {
      const gesture = active;
      const kind = gesture.edge ? (gesture.edge === 'start' ? 'trim-start' : 'trim-end') : 'move';
      const deltaSec = clipGestureDeltaSec(
        kind,
        gesture.deltaMs,
        gesture.targetMs,
        gesture.block.startMs,
        gesture.block.endMs
      );
      return [
        overlay((ctx) =>
          drawClipGhosts(
            ctx,
            gesture.clips.map((clip) =>
              ghostSpan(clip, { kind, deltaMs: gesture.deltaMs, targetMs: gesture.targetMs })
            ),
            gesture.rowTop,
            deps.waveformHeightFor(gesture.block.deck),
            deps.accentFor(gesture.block.deck),
            `${deltaSec > 0 ? '+' : ''}${deltaSec.toFixed(2)}s`,
            gesture.block.startMs + (kind === 'move' ? gesture.deltaMs : 0),
            msToX,
            viewContext.canvasW
          )
        )
      ];
    }
    return [];
  }

  function cursorFor(point: Point): string {
    const hit = hitAt(point);
    if (!hit) return '';
    if (hit.target === 'laneSeparator' || hit.target === 'waveformSeparator') return 'row-resize';
    if (hit.target === 'overview')
      return hit.part === 'resize-left' || hit.part === 'resize-right' ? 'ew-resize' : 'grab';
    if (hit.target === 'clip' || hit.target === 'clipBand') {
      if (!deps.isEditMode()) return '';
      if (hit.target === 'clip')
        return hit.part === 'start' || hit.part === 'end' ? 'ew-resize' : 'grab';
      // Empty band space rubber-band selects in edit mode.
      return 'crosshair';
    }
    if (hit.target === 'filterRegion') return hit.part === 'body' ? 'grab' : 'ew-resize';
    if (hit.target === 'laneDropdown' || hit.target === 'deckLabel') return 'pointer';
    if (hit.target === 'lane') return 'crosshair';
    return '';
  }

  function armClip(
    block: TransportBlock,
    rowTop: number,
    edge: 'start' | 'end' | null,
    viewContext: ViewContext,
    point: Point
  ): void {
    const allClips = deps.getClips();
    const bounds = blockBounds(deps.getEvents(), allClips, block);
    const grabMs = viewContext.xToMs(point.x);
    const blockClips = allClips.filter(
      (clip) =>
        clip.deck === block.deck &&
        clip.sessionStartMs >= block.startMs - 1 &&
        clip.sessionEndMs <= block.endMs + 1
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
      startTrimMinMs: bounds?.startTrimMinMs ?? 0,
      minBlockMs: bounds?.minBlockMs ?? 0,
      snapMs: (viewContext.view.duration / viewContext.trackW) * EDGE_SNAP_PX
    };
  }

  // Edge magnetism, not beat snapping: an edge locks onto a neighbour or its own
  // original position, so placing a clip by eye is sample-exact.
  function updateClip(gesture: Extract<ActiveGesture, { kind: 'clip' }>, pointerMs: number): void {
    const { block, snapMs } = gesture;
    if (!gesture.edge) {
      let delta = pointerMs - gesture.grabMs;
      const rawStart = block.startMs + delta;
      const snappedStart = snapToEdges(rawStart, [block.startMs, gesture.minStartMs], snapMs);
      if (snappedStart !== rawStart) {
        delta = snappedStart - block.startMs;
      } else {
        const rawEnd = block.endMs + delta;
        const snappedEnd = snapToEdges(rawEnd, [gesture.maxEndMs], snapMs);
        if (snappedEnd !== rawEnd) delta = snappedEnd - block.endMs;
      }
      gesture.deltaMs = Math.max(
        gesture.minStartMs - block.startMs,
        Math.min(gesture.maxEndMs - block.endMs, delta)
      );
      return;
    }
    if (gesture.edge === 'start') {
      const target = snapToEdges(pointerMs, [block.startMs, gesture.minStartMs], snapMs);
      gesture.targetMs = Math.max(
        gesture.startTrimMinMs,
        Math.min(block.endMs - gesture.minBlockMs, target)
      );
    } else {
      const target = snapToEdges(pointerMs, [block.endMs, gesture.maxEndMs], snapMs);
      gesture.targetMs = Math.max(
        block.startMs + gesture.minBlockMs,
        Math.min(gesture.maxEndMs, target)
      );
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

function overlay(draw: (ctx: CanvasRenderingContext2D) => void): SceneItem {
  return {
    bounds: (viewContext) => ({ x: 0, y: 0, w: viewContext.canvasW, h: viewContext.canvasH }),
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

function overviewDrag(
  gesture: Extract<ActiveGesture, { kind: 'overview' }>,
  frac: number,
  total: number
): { start: number; duration: number } {
  const ms = frac * (total || 1);
  if (gesture.mode === 'resize-left')
    return resizeLeftEdge(gesture.startView, ms, total || 1, MIN_VIEW_MS);
  if (gesture.mode === 'resize-right')
    return resizeRightEdge(gesture.startView, ms, total || 1, MIN_VIEW_MS);
  // Carries the pointer's offset within the rectangle, so grabbing it off-centre
  // does not snap its middle to the cursor.
  return panByMs(
    gesture.startView,
    (frac - gesture.grabFrac) * (total || 1),
    total || 1,
    MIN_VIEW_MS
  );
}
