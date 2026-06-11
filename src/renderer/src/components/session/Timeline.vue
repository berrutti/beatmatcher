<template>
  <div class="timeline" ref="containerEl">
    <canvas
      ref="canvasEl"
      class="timeline__canvas"
      @click="onCanvasClick"
      @contextmenu.prevent="onCanvasContextMenu"
      @wheel.prevent="onWheel"
      @mousedown="onCanvasMouseDown"
      @mousemove="onCanvasHoverMove"
      @mouseleave="onCanvasHoverLeave"
    />
  </div>

  <Teleport to="body">
    <div
      v-if="laneMenu"
      class="lane-menu"
      :style="{ left: laneMenu.x + 'px', top: laneMenu.y + 'px' }"
      @click.stop
    >
      <button
        v-for="key in LANE_KEYS"
        :key="key"
        class="lane-menu__item"
        @click="toggleLane(laneMenu.deck, key)"
      >
        <span class="lane-menu__check">{{ isLaneVisible(laneMenu.deck, key) ? '✓' : '' }}</span>
        {{ $t(`session.lanes.${key}`) }}
      </button>
    </div>
    <div
      v-if="laneMenu"
      class="lane-menu__backdrop"
      @click="laneMenu = null"
      @contextmenu.prevent="laneMenu = null"
    />
  </Teleport>
</template>

<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from 'vue';
import type {
  Clip,
  LoadedSpan,
  DeckLanes,
  MasterLanes,
  NudgeSpan
} from '@renderer/composables/useSessionTimeline';
import type { TrackWaveform } from '@renderer/utils/timelineDraw';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import {
  type ViewWindow,
  clampView,
  zoomAroundCursor,
  panByMs,
  recenterOn,
  resizeLeftEdge,
  resizeRightEdge,
  followTarget,
  overlapsRange,
  fracToMs,
  clampFrac,
  hitTestOverview,
  chooseTickInterval
} from '@renderer/utils/timelineView';
import {
  DECK_ORDER,
  LANE_KEYS,
  LANE_SHORT_LABELS,
  ROW_H,
  LABEL_W,
  TICK_H,
  PADDING,
  SUBLANE_H,
  MASTER_ROW_H,
  OVERVIEW_H,
  OVERVIEW_GAP,
  type LaneKey,
  type LaneVisibility,
  type RowLayout,
  type SublaneLayout,
  type OverviewRect,
  formatTickLabel,
  drawLoadedSpan,
  drawLoadedSpanLabel,
  drawClip,
  drawNudgeSpans,
  drawDeckLanes,
  drawMasterGainLane,
  drawOverview,
  makeMsToX,
  valueToY,
  yToValue
} from '@renderer/utils/timelineDraw';
import { DECK_ACCENTS, DeckId, useDecksStore } from '@renderer/stores/decks';
import { useSessionStore } from '@renderer/stores/session';
import { useSessionEditStore } from '@renderer/stores/sessionEdit';
import { useSettingsStore } from '@renderer/stores/settings';
import {
  laneSpecFor,
  normalizeGestureSamples,
  formatLaneValue,
  filterActiveAt,
  type EditableLaneKey
} from '@renderer/utils/sessionEditOps';
import {
  blocksForDeck,
  blockBounds,
  MIN_BLOCK_MS,
  type TransportBlock
} from '@renderer/utils/clipEditOps';
import type { LanePoint } from '@renderer/composables/useSessionTimeline';

type DragMode =
  | 'track'
  | 'overview-move'
  | 'overview-resize-left'
  | 'overview-resize-right'
  | 'draw'
  | 'paint-active'
  | 'paint-nudge'
  | 'clip'
  | null;
type DragState = { mode: DragMode; startClientX: number; startView: ViewWindow; dragged: boolean };
type LaneMenu = { deck: string; x: number; y: number };
type LaneHit = { deck: string; lane: EditableLaneKey; top: number; height: number };
type DrawGesture = {
  deck: string;
  lane: EditableLaneKey;
  top: number;
  height: number;
  min: number;
  max: number;
  samples: LanePoint[];
};
type PaintGesture = {
  deck: string;
  top: number;
  height: number;
  startMs: number;
  currentMs: number;
};
type NudgeGesture = {
  deck: string;
  rowTop: number;
  direction: 1 | -1;
  startMs: number;
  currentMs: number;
};
type ClipGesture = {
  block: TransportBlock;
  blockClips: Clip[];
  kind: 'move' | 'trim-start' | 'trim-end';
  grabMs: number;
  deltaMs: number;
  targetMs: number;
  beatMs: number | null;
  minStartMs: number;
  maxEndMs: number;
};

const MIN_VIEW_MS = 200;
const ZOOM_SENSITIVITY = 0.0015;
const DRAG_THRESHOLD_PX = 3;
const EDGE_GRAB_PX = 6;
const FOLLOW_LEAD_IN_FRACTION = 0.1;
const EDIT_LANE_H = 64;

const props = defineProps<{
  durationMs: number;
  clips: Clip[];
  loadedSpans: LoadedSpan[];
  playheadMs: number;
  deckLanes: Record<string, DeckLanes>;
  masterLanes: MasterLanes;
  deckNudges: Record<string, NudgeSpan[]>;
  waveforms: Map<string, TrackWaveform>;
  bpmForPath: (path: string) => number | null;
}>();

const emit = defineEmits<{ seek: [ms: number] }>();

const decks = useDecksStore();
const sessionStore = useSessionStore();
const editStore = useSessionEditStore();
const settingsStore = useSettingsStore();

function getDeckAccent(id: DeckId): string {
  return decks.decks[id]?.accent ?? DECK_ACCENTS[id];
}

const viewStartMs = ref(0);
const viewDurationMs = ref(1);

const dragState: DragState = {
  mode: null,
  startClientX: 0,
  startView: { start: 0, duration: 1 },
  dragged: false
};

function currentView(): ViewWindow {
  return { start: viewStartMs.value, duration: viewDurationMs.value };
}

function fullView(): ViewWindow {
  return { start: 0, duration: props.durationMs || 1 };
}

function setView(next: ViewWindow) {
  viewStartMs.value = next.start;
  viewDurationMs.value = next.duration;
}

watch(
  () => props.durationMs,
  (d) => setView(clampView(0, d || 1, d || 1, MIN_VIEW_MS)),
  { immediate: true }
);

function trackWidthOf(rect: DOMRect): number {
  return rect.width - LABEL_W - PADDING;
}

function fracAtClientX(clientX: number, rect: DOMRect, trackW: number): number {
  return clampFrac((clientX - rect.left - LABEL_W) / trackW);
}

function onWheel(e: WheelEvent) {
  if (!props.durationMs || !canvasEl.value) return;
  const rect = canvasEl.value.getBoundingClientRect();
  const trackW = trackWidthOf(rect);
  if (trackW <= 0) return;

  const total = props.durationMs;
  const view = currentView();

  if (e.ctrlKey || e.metaKey) {
    const frac = fracAtClientX(e.clientX, rect, trackW);
    setView(zoomAroundCursor(view, frac, e.deltaY, ZOOM_SENSITIVITY, total, MIN_VIEW_MS));
  } else {
    const deltaMs = (e.deltaX || e.deltaY) * (view.duration / trackW);
    setView(panByMs(view, deltaMs, total, MIN_VIEW_MS));
  }
}

function isOverY(y: number): boolean {
  return overviewRect !== null && y >= overviewRect.y && y < overviewRect.y + overviewRect.h;
}

function onCanvasMouseDown(e: MouseEvent) {
  if (!props.durationMs || !canvasEl.value) return;
  const rect = canvasEl.value.getBoundingClientRect();
  const trackW = trackWidthOf(rect);
  if (trackW <= 0) return;
  const y = e.clientY - rect.top;
  const frac = fracAtClientX(e.clientX, rect, trackW);

  if (editStore.editMode && !isOverY(y) && e.clientX - rect.left > LABEL_W) {
    if (e.shiftKey) {
      const band = clipBandAt(y);
      if (band) {
        const ms = fracToMs(frac, currentView());
        nudgeGesture = {
          deck: band.deck,
          rowTop: band.rowTop,
          direction: band.direction,
          startMs: ms,
          currentMs: ms
        };
        dragState.mode = 'paint-nudge';
        dragState.startClientX = e.clientX;
        dragState.startView = currentView();
        dragState.dragged = false;
        canvasEl.value.style.cursor = 'crosshair';
        window.addEventListener('mousemove', onWindowMouseMove);
        window.addEventListener('mouseup', onWindowMouseUp);
        requestAnimationFrame(draw);
        return;
      }
    }
    if (!e.shiftKey) {
      const clipHit = blockAtPoint(e.clientX - rect.left, y, trackW);
      if (clipHit && beginClipGesture(clipHit, fracToMs(frac, currentView()))) {
        dragState.mode = 'clip';
        dragState.startClientX = e.clientX;
        dragState.startView = currentView();
        dragState.dragged = false;
        canvasEl.value.style.cursor = clipHit.edge ? 'ew-resize' : 'grabbing';
        window.addEventListener('mousemove', onWindowMouseMove);
        window.addEventListener('mouseup', onWindowMouseUp);
        return;
      }
    }
    const hit = laneAt(y);
    if (hit && isLaneSelected(hit.deck, hit.lane)) {
      const ms = fracToMs(frac, currentView());
      if (e.shiftKey && hit.lane === 'filter') {
        paintGesture = {
          deck: hit.deck,
          top: hit.top,
          height: hit.height,
          startMs: ms,
          currentMs: ms
        };
        dragState.mode = 'paint-active';
      } else {
        beginDrawGesture(hit, ms, y);
        dragState.mode = 'draw';
      }
      dragState.startClientX = e.clientX;
      dragState.startView = currentView();
      dragState.dragged = false;
      canvasEl.value.style.cursor = 'crosshair';
      window.addEventListener('mousemove', onWindowMouseMove);
      window.addEventListener('mouseup', onWindowMouseUp);
      requestAnimationFrame(draw);
      return;
    }
  }

  let mode: DragMode = 'track';

  if (isOverY(y)) {
    const total = props.durationMs;
    const edgeTolerance = EDGE_GRAB_PX / trackW;
    const hit = hitTestOverview(frac, currentView(), total, edgeTolerance);
    switch (hit) {
      case 'resize-left':
        mode = 'overview-resize-left';
        break;
      case 'resize-right':
        mode = 'overview-resize-right';
        break;
      case 'move':
        mode = 'overview-move';
        break;
      case 'outside':
        mode = 'overview-move';
        setView(recenterOn(currentView(), fracToMs(frac, fullView()), total, MIN_VIEW_MS));
        break;
    }
  }

  dragState.mode = mode;
  dragState.startClientX = e.clientX;
  dragState.startView = currentView();
  dragState.dragged = false;

  canvasEl.value.style.cursor =
    mode === 'overview-resize-left' || mode === 'overview-resize-right' ? 'ew-resize' : 'grabbing';

  window.addEventListener('mousemove', onWindowMouseMove);
  window.addEventListener('mouseup', onWindowMouseUp);
}

function onCanvasHoverMove(e: MouseEvent) {
  if (dragState.mode || !canvasEl.value || !props.durationMs) return;
  const rect = canvasEl.value.getBoundingClientRect();
  const trackW = trackWidthOf(rect);
  if (trackW <= 0) return;
  const y = e.clientY - rect.top;

  if (isOverY(y)) {
    const frac = fracAtClientX(e.clientX, rect, trackW);
    const edgeTolerance = EDGE_GRAB_PX / trackW;
    const hit = hitTestOverview(frac, currentView(), props.durationMs, edgeTolerance);
    canvasEl.value.style.cursor =
      hit === 'resize-left' || hit === 'resize-right' ? 'ew-resize' : 'grab';
  } else if (editStore.editMode) {
    if (e.shiftKey && clipBandAt(y) && e.clientX - rect.left > LABEL_W) {
      canvasEl.value.style.cursor = 'crosshair';
      return;
    }
    if (!e.shiftKey && e.clientX - rect.left > LABEL_W) {
      const clipHit = blockAtPoint(e.clientX - rect.left, y, trackW);
      if (clipHit) {
        canvasEl.value.style.cursor = clipHit.edge ? 'ew-resize' : 'grab';
        return;
      }
    }
    const hit = laneAt(y);
    if (hit && isLaneSelected(hit.deck, hit.lane) && e.clientX - rect.left > LABEL_W) {
      canvasEl.value.style.cursor = 'crosshair';
    } else {
      canvasEl.value.style.cursor = hit ? 'pointer' : '';
    }
  } else {
    canvasEl.value.style.cursor = '';
  }
}

function onCanvasHoverLeave() {
  if (!dragState.mode && canvasEl.value) canvasEl.value.style.cursor = '';
}

function onWindowMouseMove(e: MouseEvent) {
  if (!dragState.mode || !canvasEl.value) return;
  const rect = canvasEl.value.getBoundingClientRect();
  const trackW = trackWidthOf(rect);
  if (trackW <= 0) return;

  const dxPx = e.clientX - dragState.startClientX;
  if (Math.abs(dxPx) > DRAG_THRESHOLD_PX) dragState.dragged = true;

  const total = props.durationMs || 1;
  const start = dragState.startView;

  switch (dragState.mode) {
    case 'paint-active': {
      if (!paintGesture) break;
      const frac = fracAtClientX(e.clientX, rect, trackW);
      paintGesture.currentMs = Math.min(total, Math.max(0, fracToMs(frac, start)));
      requestAnimationFrame(draw);
      break;
    }
    case 'paint-nudge': {
      if (!nudgeGesture) break;
      const frac = fracAtClientX(e.clientX, rect, trackW);
      nudgeGesture.currentMs = Math.min(total, Math.max(0, fracToMs(frac, start)));
      requestAnimationFrame(draw);
      break;
    }
    case 'clip': {
      // Edits happen while stopped; the press itself may still be a scrub
      // click, so playback stops only once an actual drag begins.
      if (dragState.dragged && sessionStore.isPlaying) sessionStore.stop().catch(() => {});
      const frac = fracAtClientX(e.clientX, rect, trackW);
      const pointerMs = Math.min(total, Math.max(0, fracToMs(frac, start)));
      // Cmd/Ctrl disables beat snapping for free placement.
      updateClipGesture(pointerMs, e.metaKey || e.ctrlKey);
      requestAnimationFrame(draw);
      break;
    }
    case 'draw': {
      if (!drawGesture) break;
      const frac = fracAtClientX(e.clientX, rect, trackW);
      const ms = Math.min(total, Math.max(0, fracToMs(frac, start)));
      const y = e.clientY - rect.top;
      // Holding shift locks the value at the previous sample so the line
      // extends perfectly flat; only time advances.
      const lastSample = drawGesture.samples[drawGesture.samples.length - 1];
      const value =
        e.shiftKey && lastSample
          ? lastSample.value
          : yToValue(drawGesture.top, drawGesture.height, drawGesture.min, drawGesture.max, y);
      drawGesture.samples.push({ ms, value });
      requestAnimationFrame(draw);
      break;
    }
    case 'track': {
      const deltaMs = -dxPx * (start.duration / trackW);
      setView(panByMs(start, deltaMs, total, MIN_VIEW_MS));
      break;
    }
    case 'overview-move': {
      const deltaMs = dxPx * (total / trackW);
      setView(panByMs(start, deltaMs, total, MIN_VIEW_MS));
      break;
    }
    case 'overview-resize-left': {
      const frac = fracAtClientX(e.clientX, rect, trackW);
      setView(resizeLeftEdge(start, fracToMs(frac, fullView()), total, MIN_VIEW_MS));
      break;
    }
    case 'overview-resize-right': {
      const frac = fracAtClientX(e.clientX, rect, trackW);
      setView(resizeRightEdge(start, fracToMs(frac, fullView()), total, MIN_VIEW_MS));
      break;
    }
  }
}

function onWindowMouseUp() {
  if (dragState.mode === 'draw') {
    finishDrawGesture();
    // Suppress the click event that follows this mouseup so it cannot seek.
    dragState.dragged = true;
  }
  if (dragState.mode === 'paint-active') {
    finishPaintGesture();
    dragState.dragged = true;
  }
  if (dragState.mode === 'paint-nudge') {
    finishNudgeGesture();
    dragState.dragged = true;
  }
  if (dragState.mode === 'clip') {
    finishClipGesture();
  }
  dragState.mode = null;
  if (canvasEl.value) canvasEl.value.style.cursor = '';
  window.removeEventListener('mousemove', onWindowMouseMove);
  window.removeEventListener('mouseup', onWindowMouseUp);
}

function onCanvasClick(e: MouseEvent) {
  if (dragState.dragged) {
    dragState.dragged = false;
    return;
  }
  if (!props.durationMs || !canvasEl.value) return;
  const rect = canvasEl.value.getBoundingClientRect();
  const y = e.clientY - rect.top;
  if (isOverY(y)) return;

  if (editStore.editMode) {
    const hit = laneAt(y);
    if (hit) {
      if (!isLaneSelected(hit.deck, hit.lane)) {
        editStore.selectedLane = { deck: hit.deck, lane: hit.lane };
      } else if (e.clientX - rect.left <= LABEL_W) {
        editStore.selectedLane = null;
      }
      return;
    }
  }

  const trackW = trackWidthOf(rect);
  if (trackW <= 0) return;
  const frac = fracAtClientX(e.clientX, rect, trackW);
  emit('seek', fracToMs(frac, currentView()));
}

const laneVisibility = ref<Record<string, LaneVisibility>>(
  storageGet(STORAGE_KEYS.sessionLaneVisibility, {})
);

function isLaneVisible(deck: string, lane: LaneKey): boolean {
  return laneVisibility.value[deck]?.[lane] ?? true;
}

function visibleLanesFor(deck: string): LaneKey[] {
  return LANE_KEYS.filter((lane) => isLaneVisible(deck, lane));
}

function toggleLane(deck: string, lane: LaneKey) {
  const current = laneVisibility.value[deck] ?? {};
  laneVisibility.value = {
    ...laneVisibility.value,
    [deck]: { ...current, [lane]: !isLaneVisible(deck, lane) }
  };
  storageSet(STORAGE_KEYS.sessionLaneVisibility, laneVisibility.value);
}

const laneMenu = ref<LaneMenu | null>(null);

function onCanvasContextMenu(e: MouseEvent) {
  if (!canvasEl.value) return;
  const rect = canvasEl.value.getBoundingClientRect();
  const y = e.clientY - rect.top;
  const row = rowLayout.find((r) => y >= r.top && y < r.top + r.height);
  if (!row) return;
  laneMenu.value = { deck: row.deckId, x: e.clientX, y: e.clientY };
}

let rowLayout: RowLayout[] = [];
let overviewRect: OverviewRect | null = null;
let masterRect: { top: number; height: number } | null = null;
let drawGesture: DrawGesture | null = null;
let paintGesture: PaintGesture | null = null;
let nudgeGesture: NudgeGesture | null = null;
let clipGesture: ClipGesture | null = null;

// The clip band is the waveform strip of a deck row, above its sublanes.
function clipBandAt(y: number): { deck: string; rowTop: number; direction: 1 | -1 } | null {
  for (const row of rowLayout) {
    if (y >= row.top && y < row.top + ROW_H) {
      return { deck: row.deckId, rowTop: row.top, direction: y < row.top + ROW_H / 2 ? 1 : -1 };
    }
  }
  return null;
}

// Finds the transport block (and edge, for trims) under a point on a deck's
// clip band. Loop blocks expose no edges: they move as one unit only.
function blockAtPoint(
  x: number,
  y: number,
  trackW: number
): { block: TransportBlock; edge: 'start' | 'end' | null } | null {
  const band = clipBandAt(y);
  if (!band) return null;
  const msToX = makeMsToX(currentView(), trackW);
  for (const block of blocksForDeck(props.clips, band.deck)) {
    const x0 = msToX(block.startMs);
    const x1 = msToX(block.endMs);
    if (x < x0 - EDGE_GRAB_PX || x > x1 + EDGE_GRAB_PX) continue;
    if (!block.loop) {
      if (Math.abs(x - x0) <= EDGE_GRAB_PX) return { block, edge: 'start' };
      if (Math.abs(x - x1) <= EDGE_GRAB_PX) return { block, edge: 'end' };
    }
    if (x >= x0 && x <= x1) return { block, edge: null };
  }
  return null;
}

function beginClipGesture(hit: { block: TransportBlock; edge: 'start' | 'end' | null }, ms: number) {
  const events = sessionStore.session?.events ?? [];
  const bounds = blockBounds(events, props.clips, hit.block);
  if (!bounds) return false;
  const bpm = props.bpmForPath(hit.block.trackPath);
  const blockClips = props.clips.filter(
    (clip) =>
      clip.deck === hit.block.deck &&
      clip.sessionStartMs >= hit.block.startMs - 1 &&
      clip.sessionEndMs <= hit.block.endMs + 1
  );
  clipGesture = {
    block: hit.block,
    blockClips,
    kind: hit.edge ? (hit.edge === 'start' ? 'trim-start' : 'trim-end') : 'move',
    grabMs: ms,
    deltaMs: 0,
    targetMs: hit.edge === 'start' ? hit.block.startMs : hit.block.endMs,
    beatMs: bpm !== null && bpm > 0 ? 60_000 / (bpm * hit.block.playbackRate) : null,
    minStartMs: bounds.minStartMs,
    maxEndMs: bounds.maxEndMs
  };
  return true;
}

function updateClipGesture(pointerMs: number, free: boolean) {
  if (!clipGesture) return;
  const { block, beatMs } = clipGesture;
  if (clipGesture.kind === 'move') {
    let delta = pointerMs - clipGesture.grabMs;
    if (!free && beatMs) delta = Math.round(delta / beatMs) * beatMs;
    clipGesture.deltaMs = Math.max(
      clipGesture.minStartMs - block.startMs,
      Math.min(clipGesture.maxEndMs - block.endMs, delta)
    );
    return;
  }
  const anchor = clipGesture.kind === 'trim-start' ? block.startMs : block.endMs;
  let target = pointerMs;
  if (!free && beatMs) target = anchor + Math.round((target - anchor) / beatMs) * beatMs;
  if (clipGesture.kind === 'trim-start') {
    const earliestByAudio = block.startMs - (block.trackStartSec / block.playbackRate) * 1000;
    clipGesture.targetMs = Math.max(
      Math.max(clipGesture.minStartMs, earliestByAudio),
      Math.min(block.endMs - MIN_BLOCK_MS, target)
    );
  } else {
    clipGesture.targetMs = Math.max(
      block.startMs + MIN_BLOCK_MS,
      Math.min(clipGesture.maxEndMs, target)
    );
  }
}

function finishClipGesture() {
  if (!clipGesture) return;
  const gesture = clipGesture;
  clipGesture = null;
  // A press without movement is a click (scrub), not an edit.
  if (!dragState.dragged) {
    requestAnimationFrame(draw);
    return;
  }
  if (gesture.kind === 'move') {
    if (Math.abs(gesture.deltaMs) >= 1) {
      editStore.commitClipMove(props.clips, gesture.block, gesture.deltaMs).catch(() => {});
    }
  } else {
    const edge = gesture.kind === 'trim-start' ? 'start' : 'end';
    editStore.commitClipTrim(props.clips, gesture.block, edge, gesture.targetMs).catch(() => {});
  }
  requestAnimationFrame(draw);
}

function finishNudgeGesture() {
  if (!nudgeGesture) return;
  const gesture = nudgeGesture;
  nudgeGesture = null;
  const t0 = Math.min(gesture.startMs, gesture.currentMs);
  const t1 = Math.max(gesture.startMs, gesture.currentMs);
  editStore.commitNudgePaint(gesture.deck, t0, t1, gesture.direction).catch(() => {});
  requestAnimationFrame(draw);
}

function isLaneSelected(deck: string, lane: EditableLaneKey): boolean {
  const sel = editStore.selectedLane;
  return editStore.editMode && sel !== null && sel.deck === deck && sel.lane === lane;
}

function laneHeightFor(deck: string, lane: LaneKey): number {
  return isLaneSelected(deck, lane) ? EDIT_LANE_H : SUBLANE_H;
}

function laneAt(y: number): LaneHit | null {
  for (const row of rowLayout) {
    for (const sub of row.lanes) {
      if (y >= sub.top && y < sub.top + sub.height) {
        return { deck: row.deckId, lane: sub.key, top: sub.top, height: sub.height };
      }
    }
  }
  if (masterRect && y >= masterRect.top && y < masterRect.top + masterRect.height) {
    // The master lane draws inset by 2px (see drawMasterGainLane), so value
    // mapping uses the same inset rect.
    return {
      deck: 'master',
      lane: 'masterGain',
      top: masterRect.top + 2,
      height: masterRect.height - 4
    };
  }
  return null;
}

function laneRange(hit: LaneHit): { min: number; max: number } {
  const deckData = props.deckLanes[hit.deck];
  const spec = laneSpecFor(hit.lane, { rateMin: deckData?.rateMin, rateMax: deckData?.rateMax });
  return { min: spec.min, max: spec.max };
}

function beginDrawGesture(hit: LaneHit, ms: number, y: number) {
  if (sessionStore.isPlaying) sessionStore.stop().catch(() => {});
  const { min, max } = laneRange(hit);
  const value = yToValue(hit.top, hit.height, min, max, y);
  drawGesture = {
    deck: hit.deck === 'master' ? '' : hit.deck,
    lane: hit.lane,
    top: hit.top,
    height: hit.height,
    min,
    max,
    samples: [{ ms, value }]
  };
}

function finishPaintGesture() {
  if (!paintGesture) return;
  const gesture = paintGesture;
  paintGesture = null;
  const t0 = Math.min(gesture.startMs, gesture.currentMs);
  const t1 = Math.max(gesture.startMs, gesture.currentMs);
  editStore.commitFilterActiveToggle(gesture.deck, t0, t1).catch(() => {});
  requestAnimationFrame(draw);
}

function finishDrawGesture() {
  if (!drawGesture || drawGesture.samples.length === 0) {
    drawGesture = null;
    return;
  }
  const gesture = drawGesture;
  drawGesture = null;
  let t0 = Infinity;
  let t1 = -Infinity;
  for (const sample of gesture.samples) {
    t0 = Math.min(t0, sample.ms);
    t1 = Math.max(t1, sample.ms);
  }
  editStore
    .commitGesture(gesture.deck, gesture.lane, gesture.samples, t0, t1, {
      rateMin: gesture.min,
      rateMax: gesture.max
    })
    .catch(() => {});
  requestAnimationFrame(draw);
}

const containerEl = ref<HTMLDivElement | null>(null);
const canvasEl = ref<HTMLCanvasElement | null>(null);
let ro: ResizeObserver | null = null;

function draw() {
  const canvas = canvasEl.value;
  const container = containerEl.value;
  if (!canvas || !container) return;

  const dpr = window.devicePixelRatio || 1;
  const canvasW = container.clientWidth;
  const canvasH = container.clientHeight;
  if (canvasW === 0 || canvasH === 0) return;

  canvas.width = canvasW * dpr;
  canvas.height = canvasH * dpr;
  canvas.style.width = canvasW + 'px';
  canvas.style.height = canvasH + 'px';

  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.scale(dpr, dpr);

  const trackW = canvasW - LABEL_W - PADDING;
  const totalMs = props.durationMs || 1;
  const view = currentView();
  const viewStart = view.start;
  const viewDur = view.duration;
  const viewEnd = viewStart + viewDur;
  const msToX = makeMsToX(view, trackW);

  function overlapsView(startMs: number, endMs: number): boolean {
    return overlapsRange(startMs, endMs, viewStart, viewEnd);
  }

  // Background
  ctx.fillStyle = 'var(--color-bg, #111)';
  ctx.fillRect(0, 0, canvasW, canvasH);

  // Fill tick-row gutters so they match the surrounding row background color
  ctx.fillStyle = '#161616';
  ctx.fillRect(0, 0, LABEL_W, TICK_H);
  ctx.fillRect(canvasW - PADDING, 0, PADDING, TICK_H);

  // Tick marks + time labels — clipped to track area so labels don't bleed into either gutter
  const tickIntervalMs = chooseTickInterval(viewDur, trackW);
  const firstTick = Math.max(0, Math.floor(viewStart / tickIntervalMs) * tickIntervalMs);
  ctx.save();
  ctx.beginPath();
  ctx.rect(LABEL_W, 0, trackW, TICK_H);
  ctx.clip();
  ctx.font = `9px monospace`;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'alphabetic';
  for (let ms = firstTick; ms <= viewEnd; ms += tickIntervalMs) {
    const tickX = msToX(ms);
    ctx.fillStyle = '#333';
    ctx.fillRect(tickX, 0, 1, TICK_H);
    ctx.fillStyle = '#555';
    ctx.fillText(formatTickLabel(ms, tickIntervalMs), tickX + 3, TICK_H - 3);
  }
  ctx.restore();

  // Deck rows — first pass: backgrounds and labels (full width, no clip needed)
  const newRowLayout: RowLayout[] = [];
  let rowY = TICK_H;
  for (let ri = 0; ri < DECK_ORDER.length; ri++) {
    const deckId = DECK_ORDER[ri];
    const laneKeys = visibleLanesFor(deckId);
    let sublaneTop = rowY + ROW_H;
    const lanes: SublaneLayout[] = laneKeys.map((key) => {
      const sublane = { key, top: sublaneTop, height: laneHeightFor(deckId, key) };
      sublaneTop += sublane.height;
      return sublane;
    });
    const rowH = sublaneTop - rowY;
    newRowLayout.push({ deckId: deckId, top: rowY, height: rowH, lanes });

    ctx.fillStyle = ri % 2 === 0 ? '#161616' : '#131313';
    ctx.fillRect(0, rowY, canvasW, rowH);

    ctx.font = `bold 9px monospace`;
    ctx.fillStyle = getDeckAccent(deckId);
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(deckId, LABEL_W / 2, rowY + ROW_H / 2);

    if (lanes.length > 0) {
      ctx.font = `8px monospace`;
      for (const sublane of lanes) {
        ctx.fillStyle = isLaneSelected(deckId, sublane.key) ? '#06b6d4' : '#555';
        ctx.fillText(LANE_SHORT_LABELS[sublane.key], LABEL_W / 2, sublane.top + sublane.height / 2);
      }
    }

    rowY += rowH;
  }

  // Master row background + label (outside clip so label at LABEL_W/2 is not hidden)
  const masterTopY = rowY;
  const masterRowH = isLaneSelected('master', 'masterGain') ? EDIT_LANE_H : MASTER_ROW_H;
  masterRect = { top: masterTopY, height: masterRowH };
  ctx.fillStyle = '#101010';
  ctx.fillRect(0, masterTopY, canvasW, masterRowH);

  ctx.font = `bold 9px monospace`;
  ctx.fillStyle = '#888';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText('M', LABEL_W / 2, masterTopY + masterRowH / 2);

  // Second pass: track content clipped to [LABEL_W, trackW] so it never bleeds into the label column
  ctx.save();
  ctx.beginPath();
  ctx.rect(LABEL_W, 0, trackW, canvasH);
  ctx.clip();

  rowY = TICK_H;
  for (let ri = 0; ri < DECK_ORDER.length; ri++) {
    const { deckId: deckId, height: rowH, lanes } = newRowLayout[ri];
    const accent = getDeckAccent(deckId);

    for (const span of props.loadedSpans.filter(
      (s) => s.deck === deckId && overlapsView(s.startMs, s.endMs)
    )) {
      drawLoadedSpan(ctx, span, rowY, accent, msToX);
    }

    for (const clip of props.clips.filter(
      (c) => c.deck === deckId && overlapsView(c.sessionStartMs, c.sessionEndMs)
    )) {
      drawClip(ctx, clip, props.waveforms.get(clip.trackPath), rowY, accent, msToX);
    }

    const deckNudgeSpans = (props.deckNudges[deckId] ?? []).filter((n) =>
      overlapsView(n.startMs, n.endMs)
    );
    drawNudgeSpans(ctx, deckNudgeSpans, rowY, msToX);

    if (lanes.length > 0) {
      drawDeckLanes(ctx, canvasW, msToX, props.deckLanes[deckId], lanes, viewStart, viewEnd);
    }

    rowY += rowH;
  }
  rowLayout = newRowLayout;

  drawMasterGainLane(
    ctx,
    props.masterLanes.gain,
    masterTopY,
    masterRowH,
    msToX,
    viewStart,
    viewEnd
  );
  const bottomY = masterTopY + masterRowH;

  // Selected-lane highlight + in-progress draw gesture preview
  if (editStore.editMode && editStore.selectedLane) {
    const sel = editStore.selectedLane;
    let selRect: { top: number; height: number } | null = null;
    if (sel.deck === 'master') {
      selRect = { top: masterTopY, height: masterRowH };
    } else {
      const row = newRowLayout.find((r) => r.deckId === sel.deck);
      const sub = row?.lanes.find((l) => l.key === sel.lane);
      if (sub) selRect = { top: sub.top, height: sub.height };
    }
    if (selRect) {
      ctx.fillStyle = '#06b6d414';
      ctx.fillRect(LABEL_W, selRect.top, trackW, selRect.height);
      ctx.strokeStyle = '#06b6d4';
      ctx.lineWidth = 1;
      ctx.strokeRect(LABEL_W + 0.5, selRect.top + 0.5, trackW - 1, selRect.height - 1);
    }
  }

  if (drawGesture) {
    const points = normalizeGestureSamples(drawGesture.samples);
    if (points.length > 0) {
      ctx.strokeStyle = '#ffffffcc';
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      let prevY = valueToY(
        drawGesture.top,
        drawGesture.height,
        drawGesture.min,
        drawGesture.max,
        points[0].value
      );
      ctx.moveTo(msToX(points[0].ms), prevY);
      for (let pointIdx = 1; pointIdx < points.length; pointIdx++) {
        const stepX = msToX(points[pointIdx].ms);
        const stepY = valueToY(
          drawGesture.top,
          drawGesture.height,
          drawGesture.min,
          drawGesture.max,
          points[pointIdx].value
        );
        ctx.lineTo(stepX, prevY);
        ctx.lineTo(stepX, stepY);
        prevY = stepY;
      }
      ctx.stroke();

      const cursor = drawGesture.samples[drawGesture.samples.length - 1];
      const label = formatLaneValue(drawGesture.lane, cursor.value);
      const labelX = Math.min(msToX(cursor.ms) + 8, canvasW - PADDING - 40);
      const labelY = drawGesture.top - 6;
      ctx.font = '9px monospace';
      ctx.textAlign = 'left';
      ctx.textBaseline = 'alphabetic';
      ctx.lineWidth = 3;
      ctx.strokeStyle = '#000000cc';
      ctx.lineJoin = 'round';
      ctx.strokeText(label, labelX, labelY);
      ctx.fillStyle = '#ffffff';
      ctx.fillText(label, labelX, labelY);
    }
  }

  if (nudgeGesture) {
    const t0 = Math.min(nudgeGesture.startMs, nudgeGesture.currentMs);
    const t1 = Math.max(nudgeGesture.startMs, nudgeGesture.currentMs);
    const percent = nudgeGesture.direction * settingsStore.nudgeSensitivity;
    drawNudgeSpans(ctx, [{ startMs: t0, endMs: t1, percent }], nudgeGesture.rowTop, msToX);

    const label = `${percent > 0 ? '+' : ''}${percent}%`;
    const labelX = Math.min(msToX(nudgeGesture.currentMs) + 8, canvasW - PADDING - 30);
    const labelY =
      nudgeGesture.direction > 0 ? nudgeGesture.rowTop + 12 : nudgeGesture.rowTop + ROW_H - 6;
    ctx.font = '9px monospace';
    ctx.textAlign = 'left';
    ctx.textBaseline = 'alphabetic';
    ctx.lineWidth = 3;
    ctx.strokeStyle = '#000000cc';
    ctx.lineJoin = 'round';
    ctx.strokeText(label, labelX, labelY);
    ctx.fillStyle = '#ffffff';
    ctx.fillText(label, labelX, labelY);
  }

  if (paintGesture) {
    const t0 = Math.min(paintGesture.startMs, paintGesture.currentMs);
    const t1 = Math.max(paintGesture.startMs, paintGesture.currentMs);
    const events = sessionStore.session?.events ?? [];
    const want = !filterActiveAt(events, paintGesture.deck, t0, false);
    const x0 = msToX(t0);
    const paintW = Math.max(1, msToX(t1) - x0);
    ctx.fillStyle = want ? '#ffffff30' : '#00000060';
    ctx.fillRect(x0, paintGesture.top, paintW, paintGesture.height);

    const label = want ? 'ON' : 'OFF';
    const labelX = Math.min(msToX(paintGesture.currentMs) + 8, canvasW - PADDING - 30);
    const labelY = paintGesture.top - 6;
    ctx.font = '9px monospace';
    ctx.textAlign = 'left';
    ctx.textBaseline = 'alphabetic';
    ctx.lineWidth = 3;
    ctx.strokeStyle = '#000000cc';
    ctx.lineJoin = 'round';
    ctx.strokeText(label, labelX, labelY);
    ctx.fillStyle = '#ffffff';
    ctx.fillText(label, labelX, labelY);
  }

  if (clipGesture && dragState.dragged) {
    const gesture = clipGesture;
    const row = newRowLayout.find((r) => r.deckId === gesture.block.deck);
    if (row) {
      const accent = getDeckAccent(gesture.block.deck as DeckId);
      ctx.fillStyle = accent + '50';
      ctx.strokeStyle = accent;
      ctx.lineWidth = 1;
      for (const clip of gesture.blockClips) {
        let ghostStart = clip.sessionStartMs;
        let ghostEnd = clip.sessionEndMs;
        if (gesture.kind === 'move') {
          ghostStart += gesture.deltaMs;
          ghostEnd += gesture.deltaMs;
        } else if (gesture.kind === 'trim-start') {
          ghostStart = gesture.targetMs;
        } else {
          ghostEnd = gesture.targetMs;
        }
        const ghostX = msToX(ghostStart);
        const ghostW = Math.max(1, msToX(ghostEnd) - ghostX);
        ctx.fillRect(ghostX, row.top, ghostW, ROW_H);
        ctx.strokeRect(ghostX + 0.5, row.top + 0.5, ghostW - 1, ROW_H - 1);
      }

      const deltaSec =
        gesture.kind === 'move'
          ? gesture.deltaMs / 1000
          : (gesture.targetMs -
              (gesture.kind === 'trim-start' ? gesture.block.startMs : gesture.block.endMs)) /
            1000;
      const deltaMs = deltaSec * 1000;
      const onBeatGrid =
        gesture.beatMs !== null &&
        Math.abs(deltaMs - Math.round(deltaMs / gesture.beatMs) * gesture.beatMs) < 1;
      const beats =
        gesture.beatMs !== null && onBeatGrid ? Math.round(deltaMs / gesture.beatMs) : null;
      const label =
        beats !== null
          ? `${beats > 0 ? '+' : ''}${beats} beat${Math.abs(beats) === 1 ? '' : 's'}`
          : `${deltaSec > 0 ? '+' : ''}${deltaSec.toFixed(2)}s`;
      const labelX = Math.min(
        msToX(gesture.block.startMs + (gesture.kind === 'move' ? gesture.deltaMs : 0)) + 8,
        canvasW - PADDING - 60
      );
      const labelY = row.top + 12;
      ctx.font = '9px monospace';
      ctx.textAlign = 'left';
      ctx.textBaseline = 'alphabetic';
      ctx.lineWidth = 3;
      ctx.strokeStyle = '#000000cc';
      ctx.lineJoin = 'round';
      ctx.strokeText(label, labelX, labelY);
      ctx.fillStyle = '#ffffff';
      ctx.fillText(label, labelX, labelY);
    }
  }

  rowY = TICK_H;
  for (let ri = 0; ri < DECK_ORDER.length; ri++) {
    const { deckId: deckId, height: rowH } = newRowLayout[ri];
    for (const span of props.loadedSpans.filter(
      (s) => s.deck === deckId && overlapsView(s.startMs, s.endMs)
    )) {
      drawLoadedSpanLabel(ctx, span, rowY, msToX);
    }
    rowY += rowH;
  }

  ctx.restore();

  // Deck row dividers — drawn full-width (including label column) after restoring
  // the clip. Deliberately heavier than the 1px sublane-group separators so deck
  // boundaries stand out: a dark gap with a bright hairline on top.
  for (const row of newRowLayout) {
    const dividerY = row.top + row.height - 3;
    ctx.fillStyle = '#000';
    ctx.fillRect(0, dividerY, canvasW, 3);
    ctx.fillStyle = '#5a5a5a';
    ctx.fillRect(0, dividerY, canvasW, 1);
  }

  ctx.fillStyle = '#222';
  ctx.fillRect(0, masterTopY + masterRowH - 1, canvasW, 1);

  const overviewY = canvasH - OVERVIEW_H;
  const rowsBottom = Math.min(bottomY, overviewY - OVERVIEW_GAP);

  if (props.playheadMs > 0 && overlapsView(props.playheadMs, props.playheadMs)) {
    const playheadX = msToX(props.playheadMs);
    ctx.strokeStyle = '#ffffff';
    ctx.lineWidth = 1.5;
    ctx.globalAlpha = 0.9;
    ctx.beginPath();
    ctx.moveTo(playheadX, 0);
    ctx.lineTo(playheadX, rowsBottom);
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  overviewRect = drawOverview(
    ctx,
    canvasW,
    trackW,
    overviewY,
    totalMs,
    viewStart,
    viewEnd,
    props.clips,
    props.playheadMs,
    Object.fromEntries(DECK_ORDER.map((id) => [id, getDeckAccent(id)]))
  );

  ctx.fillStyle = '#2a2a2a';
  ctx.fillRect(LABEL_W - 1, 0, 1, canvasH);
  ctx.fillRect(canvasW - PADDING, 0, 1, canvasH);
}

onMounted(() => {
  ro = new ResizeObserver(() => requestAnimationFrame(draw));
  if (containerEl.value) ro.observe(containerEl.value);
  requestAnimationFrame(draw);
});

onUnmounted(() => {
  ro?.disconnect();
  window.removeEventListener('mousemove', onWindowMouseMove);
  window.removeEventListener('mouseup', onWindowMouseUp);
});

// Keeps the playhead on screen while playing: if it runs off either edge of the
// zoomed-in view, the view jumps forward/back so the playhead lands near the
// left edge with a small lead-in margin, rather than disappearing off-screen.
watch(
  () => props.playheadMs,
  (ms) => {
    const next = followTarget(
      currentView(),
      ms,
      FOLLOW_LEAD_IN_FRACTION,
      props.durationMs || 1,
      MIN_VIEW_MS
    );
    if (next) setView(next);
  }
);

watch(
  () => [
    props.clips,
    props.loadedSpans,
    props.durationMs,
    props.playheadMs,
    props.deckLanes,
    props.masterLanes,
    props.deckNudges,
    props.waveforms,
    laneVisibility.value,
    viewStartMs.value,
    viewDurationMs.value,
    editStore.editMode,
    editStore.selectedLane,
    DECK_ORDER.map((id) => getDeckAccent(id))
  ],
  () => {
    requestAnimationFrame(draw);
  }
);
</script>

<style scoped>
.timeline {
  width: 100%;
  height: 100%;
  overflow: hidden;
  position: relative;
}

.timeline__canvas {
  display: block;
}

.lane-menu__backdrop {
  position: fixed;
  inset: 0;
  z-index: 999;
}

.lane-menu {
  position: fixed;
  z-index: 1000;
  background: #1a1a1a;
  border: 1px solid #333;
  border-radius: 4px;
  padding: 4px 0;
  min-width: 140px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.6);
  font-family: var(--font);
}

.lane-menu__item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 6px 14px;
  background: none;
  border: none;
  color: var(--color-text);
  font-family: var(--font);
  font-size: 0.75rem;
  letter-spacing: 0.05em;
  text-align: left;
  cursor: pointer;
}

.lane-menu__item:hover {
  background: #2a2a2a;
  color: #fff;
}

.lane-menu__check {
  display: inline-block;
  width: 1em;
  color: #06b6d4;
}
</style>
