<template>
  <div class="timeline" ref="containerEl">
    <canvas
      ref="canvasEl"
      class="timeline__canvas"
      @click="onCanvasClick"
      @dblclick="onCanvasDblClick"
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
      <button v-if="laneMenu.nudge" class="lane-menu__item" @click="onDeleteNudge">
        <span class="lane-menu__check"></span>
        {{ $t('session.deleteNudge') }}
      </button>
      <button class="lane-menu__item" @click="onToggleMute">
        <span class="lane-menu__check">{{
          sessionStore.mutedDecks.has(laneMenu.deck) ? '✓' : ''
        }}</span>
        {{ $t('session.mute') }}
      </button>
      <button class="lane-menu__item" @click="onToggleSolo">
        <span class="lane-menu__check">{{
          sessionStore.soloDecks.has(laneMenu.deck) ? '✓' : ''
        }}</span>
        {{ $t('session.solo') }}
      </button>
      <div v-if="editStore.editMode" class="lane-menu__item lane-menu__item--sub">
        <span class="lane-menu__check"></span>
        {{ $t('session.lanesMenu') }}
        <span class="lane-menu__arrow">▶</span>
        <div class="lane-menu__submenu">
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
      </div>
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
  hitTestOverview
} from '@renderer/utils/timelineView';
import {
  DECK_ORDER,
  LANE_KEYS,
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
  type OverviewRect,
  drawTickRow,
  drawDeckRowChrome,
  drawMasterRowChrome,
  drawDeckRowContent,
  drawSelectedLaneHighlight,
  drawValueGesturePreview,
  drawNudgeGesturePreview,
  drawPaintGesturePreview,
  drawClipGhosts,
  drawLoadedSpanLabels,
  drawRowDividers,
  drawPlayhead,
  drawFrameGutters,
  drawMasterGainLane,
  drawOverview,
  makeMsToX,
  yToValue
} from '@renderer/utils/timelineDraw';
import {
  computeRowLayout,
  selectedLaneRect,
  ghostSpan,
  clipGestureDeltaSec,
  selectionSpanFor
} from '@renderer/utils/timelineLayout';
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
import { planTimelineClick } from '@renderer/utils/timelineClick';
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
type LaneMenu = { deck: string; x: number; y: number; nudge: NudgeSpan | null };
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
  minStartMs: number;
  maxEndMs: number;
  snapToleranceMs: number;
};

const MIN_VIEW_MS = 200;
const ZOOM_SENSITIVITY = 0.0015;
const DRAG_THRESHOLD_PX = 3;
const EDGE_GRAB_PX = 6;
const EDGE_SNAP_PX = 8;
const NUDGE_HIT_PX = 4;
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
      if (
        clipHit &&
        beginClipGesture(clipHit, fracToMs(frac, currentView()), currentView().duration / trackW)
      ) {
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
      updateClipGesture(pointerMs);
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
  const trackW = trackWidthOf(rect);
  if (trackW <= 0) return;

  const inLabelColumn = e.clientX - rect.left <= LABEL_W;
  const clipHit = inLabelColumn ? null : blockAtPoint(e.clientX - rect.left, y, trackW);
  const laneHit = laneAt(y);
  const plan = planTimelineClick({
    editMode: editStore.editMode,
    inLabelColumn,
    target: clipHit ? 'clip' : clipBandAt(y) ? 'clip-band' : laneHit ? 'lane' : 'background',
    laneAlreadySelected: laneHit ? isLaneSelected(laneHit.deck, laneHit.lane) : false
  });

  const ms = fracToMs(fracAtClientX(e.clientX, rect, trackW), currentView());
  if (plan.selectClip && clipHit) selectBlock(clipHit.block, ms);
  if (plan.clearClipSelection) clipSelection.value = null;
  if (plan.selectLane && laneHit) {
    editStore.selectedLane = { deck: laneHit.deck, lane: laneHit.lane };
  }
  if (plan.clearLaneSelection) editStore.selectedLane = null;
  if (plan.seek) emit('seek', ms);
}

const laneVisibility = ref<Record<string, LaneVisibility>>(
  storageGet(STORAGE_KEYS.sessionLaneVisibility, {})
);

function isLaneVisible(deck: string, lane: LaneKey): boolean {
  return laneVisibility.value[deck]?.[lane] ?? true;
}

// Lanes are an editing surface: outside edit mode only the clip bands show,
// keeping playback/render use of the session view uncluttered.
function visibleLanesFor(deck: string): LaneKey[] {
  if (!editStore.editMode) return [];
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

function nudgeSpanAt(deck: string, clientX: number, rect: DOMRect): NudgeSpan | null {
  const trackW = trackWidthOf(rect);
  if (trackW <= 0) return null;
  const ms = fracToMs(fracAtClientX(clientX, rect, trackW), currentView());
  const toleranceMs = (currentView().duration / trackW) * NUDGE_HIT_PX;
  let best: NudgeSpan | null = null;
  let bestDistMs = Infinity;
  for (const span of props.deckNudges[deck] ?? []) {
    const distMs = ms < span.startMs ? span.startMs - ms : ms > span.endMs ? ms - span.endMs : 0;
    if (distMs <= toleranceMs && distMs < bestDistMs) {
      best = span;
      bestDistMs = distMs;
    }
  }
  return best;
}

function onCanvasContextMenu(e: MouseEvent) {
  if (!canvasEl.value) return;
  const rect = canvasEl.value.getBoundingClientRect();
  const y = e.clientY - rect.top;
  const row = rowLayout.find((r) => y >= r.top && y < r.top + r.height);
  if (!row) return;
  const nudge = editStore.editMode ? nudgeSpanAt(row.deckId, e.clientX, rect) : null;
  laneMenu.value = { deck: row.deckId, x: e.clientX, y: e.clientY, nudge };
}

function onDeleteNudge() {
  const menu = laneMenu.value;
  laneMenu.value = null;
  if (!menu?.nudge) return;
  editStore.deleteNudge(menu.deck, menu.nudge.startMs, menu.nudge.endMs).catch(() => {});
}

function onToggleMute() {
  if (laneMenu.value) sessionStore.toggleMute(laneMenu.value.deck);
  laneMenu.value = null;
}

function onToggleSolo() {
  if (laneMenu.value) sessionStore.toggleSolo(laneMenu.value.deck);
  laneMenu.value = null;
}

watch(
  () => [sessionStore.mutedDecks, sessionStore.soloDecks],
  () => requestAnimationFrame(draw)
);

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
// Selection mirrors the drag unit: a whole block (loop iterations move
// together), unless the loop block has been unlocked by double-click, in
// which case single iterations are selectable.
type ClipSelection = { deck: string; blockId: number; iterationStartMs: number | null };
const clipSelection = ref<ClipSelection | null>(null);
const unlockedBlockIds = ref<Set<number>>(new Set());

// blockIds are reallocated whenever clips rebuild (any edit), so selection
// and unlocks cannot survive an edit.
watch(
  () => props.clips,
  () => {
    clipSelection.value = null;
    unlockedBlockIds.value = new Set();
  }
);

watch(
  () => editStore.editMode,
  (on) => {
    if (!on) {
      clipSelection.value = null;
      unlockedBlockIds.value = new Set();
    }
  }
);

function selectBlock(block: TransportBlock, ms: number) {
  if (block.loop && unlockedBlockIds.value.has(block.blockId)) {
    const iteration = props.clips.find(
      (clip) =>
        clip.deck === block.deck &&
        clip.blockId === block.blockId &&
        ms >= clip.sessionStartMs &&
        ms <= clip.sessionEndMs
    );
    clipSelection.value = {
      deck: block.deck,
      blockId: block.blockId,
      iterationStartMs: iteration ? iteration.sessionStartMs : null
    };
    return;
  }
  clipSelection.value = { deck: block.deck, blockId: block.blockId, iterationStartMs: null };
}

function onCanvasDblClick(e: MouseEvent) {
  if (!editStore.editMode || !props.durationMs || !canvasEl.value) return;
  const rect = canvasEl.value.getBoundingClientRect();
  const trackW = trackWidthOf(rect);
  if (trackW <= 0 || e.clientX - rect.left <= LABEL_W) return;
  const hit = blockAtPoint(e.clientX - rect.left, e.clientY - rect.top, trackW);
  if (!hit?.block.loop) return;
  const ids = new Set(unlockedBlockIds.value);
  if (ids.has(hit.block.blockId)) {
    ids.delete(hit.block.blockId);
    unlockedBlockIds.value = ids;
    clipSelection.value = {
      deck: hit.block.deck,
      blockId: hit.block.blockId,
      iterationStartMs: null
    };
  } else {
    ids.add(hit.block.blockId);
    unlockedBlockIds.value = ids;
    const ms = fracToMs(fracAtClientX(e.clientX, rect, trackW), currentView());
    selectBlock(hit.block, ms);
  }
}

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

function beginClipGesture(
  hit: { block: TransportBlock; edge: 'start' | 'end' | null },
  ms: number,
  msPerPx: number
) {
  const events = sessionStore.session?.events ?? [];
  const bounds = blockBounds(events, props.clips, hit.block);
  if (!bounds) return false;
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
    minStartMs: bounds.minStartMs,
    maxEndMs: bounds.maxEndMs,
    snapToleranceMs: msPerPx * EDGE_SNAP_PX
  };
  return true;
}

// Edge magnetism (NOT beat snapping): within a few pixels, an edge locks onto
// the neighbor block's boundary or the block's own original position. A pixel
// is hundreds of milliseconds when zoomed out, so placing a clip "touching"
// or "back where it was" by eye is otherwise never sample-exact, which is
// very audible.
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

function updateClipGesture(pointerMs: number) {
  if (!clipGesture) return;
  const { block, snapToleranceMs } = clipGesture;
  if (clipGesture.kind === 'move') {
    let delta = pointerMs - clipGesture.grabMs;
    const rawStart = block.startMs + delta;
    const snappedStart = snapToEdges(
      rawStart,
      [block.startMs, clipGesture.minStartMs],
      snapToleranceMs
    );
    if (snappedStart !== rawStart) {
      delta = snappedStart - block.startMs;
    } else {
      const rawEnd = block.endMs + delta;
      const snappedEnd = snapToEdges(rawEnd, [clipGesture.maxEndMs], snapToleranceMs);
      if (snappedEnd !== rawEnd) delta = snappedEnd - block.endMs;
    }
    clipGesture.deltaMs = Math.max(
      clipGesture.minStartMs - block.startMs,
      Math.min(clipGesture.maxEndMs - block.endMs, delta)
    );
    return;
  }
  const target =
    clipGesture.kind === 'trim-start'
      ? snapToEdges(pointerMs, [block.startMs, clipGesture.minStartMs], snapToleranceMs)
      : snapToEdges(pointerMs, [block.endMs, clipGesture.maxEndMs], snapToleranceMs);
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
  const viewEnd = viewStart + view.duration;
  const msToX = makeMsToX(view, trackW);

  // Background
  ctx.fillStyle = 'var(--color-bg, #111)';
  ctx.fillRect(0, 0, canvasW, canvasH);

  drawTickRow(ctx, canvasW, trackW, view, msToX);

  // Deck rows — first pass: backgrounds and labels (full width, no clip needed)
  const newRowLayout = computeRowLayout(
    DECK_ORDER.map((deckId) => ({
      deckId,
      laneHeights: visibleLanesFor(deckId).map((key) => ({
        key,
        height: laneHeightFor(deckId, key)
      }))
    })),
    TICK_H
  );
  for (let ri = 0; ri < newRowLayout.length; ri++) {
    const row = newRowLayout[ri];
    drawDeckRowChrome(ctx, row, canvasW, {
      zebraIndex: ri,
      accent: getDeckAccent(row.deckId),
      audible: sessionStore.deckAudible(row.deckId),
      solo: sessionStore.soloDecks.has(row.deckId),
      muted: sessionStore.mutedDecks.has(row.deckId),
      selectedLaneKey: row.lanes.find((sub) => isLaneSelected(row.deckId, sub.key))?.key ?? null
    });
  }

  // Master row background + label (outside clip so label at LABEL_W/2 is not hidden)
  const lastRow = newRowLayout[newRowLayout.length - 1];
  const masterTopY = lastRow.top + lastRow.height;
  const masterRowH = isLaneSelected('master', 'masterGain') ? EDIT_LANE_H : MASTER_ROW_H;
  masterRect = { top: masterTopY, height: masterRowH };
  drawMasterRowChrome(ctx, masterTopY, masterRowH, canvasW);

  // Second pass: track content clipped to [LABEL_W, trackW] so it never bleeds into the label column
  ctx.save();
  ctx.beginPath();
  ctx.rect(LABEL_W, 0, trackW, canvasH);
  ctx.clip();

  for (const row of newRowLayout) {
    drawDeckRowContent(
      ctx,
      row,
      {
        accent: getDeckAccent(row.deckId),
        loadedSpans: props.loadedSpans,
        clips: props.clips,
        waveforms: props.waveforms,
        nudgeSpans: props.deckNudges[row.deckId] ?? [],
        deckLanes: props.deckLanes[row.deckId],
        audible: sessionStore.deckAudible(row.deckId),
        selectionSpan: editStore.editMode
          ? selectionSpanFor(clipSelection.value, props.clips, row.deckId)
          : null
      },
      canvasW,
      trackW,
      view,
      msToX
    );
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

  // Selected-lane highlight + in-progress gesture previews
  if (editStore.editMode) {
    const selRect = selectedLaneRect(editStore.selectedLane, newRowLayout, masterRect);
    if (selRect) drawSelectedLaneHighlight(ctx, selRect, trackW);
  }

  if (drawGesture) {
    const cursor = drawGesture.samples[drawGesture.samples.length - 1];
    drawValueGesturePreview(
      ctx,
      drawGesture,
      normalizeGestureSamples(drawGesture.samples),
      formatLaneValue(drawGesture.lane, cursor.value),
      cursor.ms,
      msToX,
      canvasW
    );
  }

  if (nudgeGesture) {
    drawNudgeGesturePreview(
      ctx,
      Math.min(nudgeGesture.startMs, nudgeGesture.currentMs),
      Math.max(nudgeGesture.startMs, nudgeGesture.currentMs),
      nudgeGesture.direction * settingsStore.nudgeSensitivity,
      nudgeGesture.rowTop,
      nudgeGesture.currentMs,
      msToX,
      canvasW
    );
  }

  if (paintGesture) {
    const t0 = Math.min(paintGesture.startMs, paintGesture.currentMs);
    const want = !filterActiveAt(sessionStore.session?.events ?? [], paintGesture.deck, t0, false);
    drawPaintGesturePreview(
      ctx,
      t0,
      Math.max(paintGesture.startMs, paintGesture.currentMs),
      want,
      paintGesture.top,
      paintGesture.height,
      paintGesture.currentMs,
      msToX,
      canvasW
    );
  }

  if (clipGesture && dragState.dragged) {
    const gesture = clipGesture;
    const row = newRowLayout.find((candidate) => candidate.deckId === gesture.block.deck);
    if (row) {
      const deltaSec = clipGestureDeltaSec(
        gesture.kind,
        gesture.deltaMs,
        gesture.targetMs,
        gesture.block.startMs,
        gesture.block.endMs
      );
      drawClipGhosts(
        ctx,
        gesture.blockClips.map((clip) => ghostSpan(clip, gesture)),
        row.top,
        getDeckAccent(gesture.block.deck as DeckId),
        `${deltaSec > 0 ? '+' : ''}${deltaSec.toFixed(2)}s`,
        gesture.block.startMs + (gesture.kind === 'move' ? gesture.deltaMs : 0),
        msToX,
        canvasW
      );
    }
  }

  drawLoadedSpanLabels(ctx, newRowLayout, props.loadedSpans, view, msToX);

  ctx.restore();

  drawRowDividers(ctx, newRowLayout, canvasW);
  ctx.fillStyle = '#222';
  ctx.fillRect(0, masterTopY + masterRowH - 1, canvasW, 1);

  const overviewY = canvasH - OVERVIEW_H;
  const rowsBottom = Math.min(bottomY, overviewY - OVERVIEW_GAP);

  if (
    props.playheadMs > 0 &&
    overlapsRange(props.playheadMs, props.playheadMs, viewStart, viewEnd)
  ) {
    drawPlayhead(ctx, msToX(props.playheadMs), rowsBottom);
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

  drawFrameGutters(ctx, canvasW, canvasH);
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
    clipSelection.value,
    DECK_ORDER.map((id) => getDeckAccent(id))
  ],
  () => {
    requestAnimationFrame(draw);
  }
);
</script>

<style scoped>
.timeline {
  /* flex: 1 + min-height: 0, not height: 100%: the timeline sits in a flex
     column next to banner siblings, and a 100% height there overflows the
     body and paints over the transport bar when the window shrinks. */
  width: 100%;
  flex: 1;
  min-height: 0;
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

.lane-menu__item--sub {
  position: relative;
}

.lane-menu__arrow {
  margin-left: auto;
  font-size: 0.6rem;
  color: var(--color-muted);
}

.lane-menu__submenu {
  display: none;
  position: absolute;
  left: 100%;
  top: -5px;
  background: #1a1a1a;
  border: 1px solid #333;
  border-radius: 4px;
  padding: 4px 0;
  min-width: 140px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.6);
}

.lane-menu__item--sub:hover .lane-menu__submenu {
  display: block;
}
</style>
