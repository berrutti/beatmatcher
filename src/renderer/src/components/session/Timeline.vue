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
        {{ $t(`session.automationLanes.${key}`) }}
      </button>
    </div>
    <div
      v-if="laneMenu"
      class="lane-menu__backdrop"
      @click="closeLaneMenu"
      @contextmenu.prevent="closeLaneMenu"
    />
  </Teleport>
</template>

<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from 'vue';
import type {
  Clip,
  LoadedSpan,
  DeckAutomation,
  MasterAutomation,
  AutomationPoint,
  NudgeSpan
} from '@renderer/composables/useSessionTimeline';
import { EQ_MIN_DB, EQ_MAX_DB } from '@renderer/stores/mixer';
import { formatMs } from '@renderer/utils/time';
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
  msToFrac,
  fracToMs,
  clampFrac,
  hitTestOverview,
  chooseTickInterval,
  sliceVisiblePoints
} from '@renderer/utils/timelineView';

const DECK_ORDER = ['A', 'B', 'C', 'D'] as const;
const DECK_ACCENT: Record<string, string> = {
  A: '#3b82f6',
  B: '#f97316',
  C: '#208043',
  D: '#d631b0'
};
const ROW_H = 44;
const LABEL_W = 32;
const TICK_H = 16;
const PADDING = 12;

const MIN_VIEW_MS = 200;
const OVERVIEW_H = 22;
const OVERVIEW_GAP = 4;
const ZOOM_SENSITIVITY = 0.0015;
const DRAG_THRESHOLD_PX = 3;
const EDGE_GRAB_PX = 6;

const SUBLANE_H = 16;
const MASTER_ROW_H = SUBLANE_H * 2;
const GAIN_COLOR = '#e5e5e5';
const FILTER_DEAD_ZONE = 0.05;
const FILTER_LPF_COLOR = '#38bdf8';
const FILTER_HPF_COLOR = '#fb923c';
const FILTER_NEUTRAL_COLOR = '#666666';
const FILTER_ACTIVE_FILL = '#ffffff10';
const EQ_BAND_COLORS: Record<string, string> = { low: '#ef4444', mid: '#eab308', high: '#3b82f6' };
const NUDGE_COLOR = '#fbbf24';
const NUDGE_LINE_W = 2;

const LANE_KEYS = ['gain', 'filter', 'eqLow', 'eqMid', 'eqHigh'] as const;
type LaneKey = (typeof LANE_KEYS)[number];
const LANE_GROUP: Record<LaneKey, number> = { gain: 0, filter: 1, eqLow: 2, eqMid: 2, eqHigh: 2 };
type LaneVisibility = Partial<Record<LaneKey, boolean>>;
const LANE_SHORT_LABELS: Record<LaneKey, string> = {
  gain: 'G',
  filter: 'F',
  eqLow: 'LO',
  eqMid: 'MD',
  eqHigh: 'HI'
};

const props = defineProps<{
  durationMs: number;
  clips: Clip[];
  loadedSpans: LoadedSpan[];
  playheadMs: number;
  showAutomation: boolean;
  deckAutomation: Record<string, DeckAutomation>;
  masterAutomation: MasterAutomation;
  deckNudges: Record<string, NudgeSpan[]>;
}>();

const emit = defineEmits<{ seek: [ms: number] }>();

const viewStartMs = ref(0);
const viewDurationMs = ref(1);

function currentView(): ViewWindow {
  return { start: viewStartMs.value, duration: viewDurationMs.value };
}

// The overview strip always maps the full session across the track width,
// regardless of the current zoom — i.e. a [0, durationMs] "view".
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

type DragMode = 'track' | 'overview-move' | 'overview-resize-left' | 'overview-resize-right' | null;
type DragState = {
  mode: DragMode;
  startClientX: number;
  startView: ViewWindow;
  dragged: boolean;
};
const dragState: DragState = {
  mode: null,
  startClientX: 0,
  startView: { start: 0, duration: 1 },
  dragged: false
};

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
  const frac = fracAtClientX(e.clientX, rect, trackW);
  emit('seek', fracToMs(frac, currentView()));
}

const laneVisibility = ref<Record<string, LaneVisibility>>(
  storageGet(STORAGE_KEYS.sessionAutomationLanes, {})
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
  storageSet(STORAGE_KEYS.sessionAutomationLanes, laneVisibility.value);
}

type LaneMenu = { deck: string; x: number; y: number };
const laneMenu = ref<LaneMenu | null>(null);

function closeLaneMenu() {
  laneMenu.value = null;
}

function onCanvasContextMenu(e: MouseEvent) {
  if (!props.showAutomation || !canvasEl.value) return;
  const rect = canvasEl.value.getBoundingClientRect();
  const y = e.clientY - rect.top;
  const row = rowLayout.find((r) => y >= r.top && y < r.top + r.height);
  if (!row) return;
  laneMenu.value = { deck: row.deck, x: e.clientX, y: e.clientY };
}

type RowLayout = { deck: string; top: number; height: number; lanes: LaneKey[] };
let rowLayout: RowLayout[] = [];

type OverviewRect = { y: number; h: number };
let overviewRect: OverviewRect | null = null;

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

  function msToX(ms: number) {
    return LABEL_W + msToFrac(ms, view) * trackW;
  }

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
    const lanes = props.showAutomation ? visibleLanesFor(deckId) : [];
    const rowH = ROW_H + lanes.length * SUBLANE_H;
    newRowLayout.push({ deck: deckId, top: rowY, height: rowH, lanes });

    ctx.fillStyle = ri % 2 === 0 ? '#161616' : '#131313';
    ctx.fillRect(0, rowY, canvasW, rowH);

    ctx.font = `bold 9px monospace`;
    ctx.fillStyle = DECK_ACCENT[deckId];
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(deckId, LABEL_W / 2, rowY + ROW_H / 2);

    if (lanes.length > 0) {
      ctx.font = `8px monospace`;
      ctx.fillStyle = '#555';
      for (let laneIdx = 0; laneIdx < lanes.length; laneIdx++) {
        const sublaneTopY = rowY + ROW_H + laneIdx * SUBLANE_H;
        ctx.fillText(LANE_SHORT_LABELS[lanes[laneIdx]], LABEL_W / 2, sublaneTopY + SUBLANE_H / 2);
      }
    }

    rowY += rowH;
  }

  // Master row background + label (outside clip so label at LABEL_W/2 is not hidden)
  const masterTopY = rowY;
  if (props.showAutomation) {
    ctx.fillStyle = '#101010';
    ctx.fillRect(0, masterTopY, canvasW, MASTER_ROW_H);

    ctx.font = `bold 9px monospace`;
    ctx.fillStyle = '#888';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText('M', LABEL_W / 2, masterTopY + MASTER_ROW_H / 2);
  }

  // Second pass: track content clipped to [LABEL_W, trackW] so it never bleeds into the label column
  ctx.save();
  ctx.beginPath();
  ctx.rect(LABEL_W, 0, trackW, canvasH);
  ctx.clip();

  rowY = TICK_H;
  for (let ri = 0; ri < DECK_ORDER.length; ri++) {
    const { deck: deckId, height: rowH, lanes } = newRowLayout[ri];
    const accent = DECK_ACCENT[deckId];

    // Loaded spans (track-on-deck background)
    const deckSpans = props.loadedSpans.filter(
      (s) => s.deck === deckId && overlapsView(s.startMs, s.endMs)
    );
    for (const span of deckSpans) {
      const spanX = msToX(span.startMs);
      const spanW = Math.max(2, msToX(span.endMs) - spanX);
      const spanY = rowY + 4;
      const spanH = ROW_H - 8;

      ctx.fillStyle = accent + '18';
      ctx.fillRect(spanX, spanY, spanW, spanH);

      ctx.strokeStyle = accent + '40';
      ctx.lineWidth = 1;
      ctx.strokeRect(spanX + 0.5, spanY + 0.5, spanW - 1, spanH - 1);

      if (spanW > 40) {
        ctx.font = `9px monospace`;
        ctx.fillStyle = accent + '55';
        ctx.textAlign = 'left';
        ctx.textBaseline = 'middle';
        ctx.save();
        ctx.beginPath();
        ctx.rect(spanX + 3, spanY, spanW - 6, spanH);
        ctx.clip();
        ctx.fillText(span.trackName, Math.max(LABEL_W + 3, spanX + 3), spanY + spanH / 2);
        ctx.restore();
      }
    }

    // Clips (actual playback rectangles)
    const deckClips = props.clips.filter(
      (c) => c.deck === deckId && overlapsView(c.sessionStartMs, c.sessionEndMs)
    );
    for (const clip of deckClips) {
      const clipX = msToX(clip.sessionStartMs);
      const clipW = Math.max(2, msToX(clip.sessionEndMs) - clipX);
      const clipY = rowY + 4;
      const clipH = ROW_H - 8;

      ctx.fillStyle = accent + '55';
      ctx.fillRect(clipX, clipY, clipW, clipH);

      ctx.strokeStyle = accent + 'cc';
      ctx.lineWidth = 1;
      ctx.strokeRect(clipX + 0.5, clipY + 0.5, clipW - 1, clipH - 1);
    }

    // Nudge markers: a border drawn along the top edge of the clip row for a
    // nudge up (positive percent, faster) and the bottom edge for a nudge down
    // (negative percent, slower), spanning the discrete nudge interval.
    const deckNudgeSpans = (props.deckNudges[deckId] ?? []).filter((n) =>
      overlapsView(n.startMs, n.endMs)
    );
    if (deckNudgeSpans.length > 0) {
      const nudgeInnerY = rowY + 4;
      const nudgeInnerH = ROW_H - 8;
      ctx.fillStyle = NUDGE_COLOR;
      for (const span of deckNudgeSpans) {
        const nudgeX = msToX(span.startMs);
        const nudgeW = Math.max(2, msToX(span.endMs) - nudgeX);
        const nudgeLineY =
          span.percent > 0 ? nudgeInnerY - NUDGE_LINE_W : nudgeInnerY + nudgeInnerH;
        ctx.fillRect(nudgeX, nudgeLineY, nudgeW, NUDGE_LINE_W);
      }
    }

    if (lanes.length > 0) {
      drawDeckAutomation(
        ctx,
        canvasW,
        rowY + ROW_H,
        msToX,
        props.deckAutomation[deckId],
        lanes,
        viewStart,
        viewEnd
      );
    }

    rowY += rowH;
  }
  rowLayout = newRowLayout;

  let bottomY = rowY;

  if (props.showAutomation) {
    drawAutomationSteps(
      ctx,
      props.masterAutomation.gain,
      masterTopY + 2,
      MASTER_ROW_H - 4,
      0,
      1,
      GAIN_COLOR,
      msToX,
      viewStart,
      viewEnd
    );
    bottomY = masterTopY + MASTER_ROW_H;
  }

  ctx.restore();

  // Deck row dividers — drawn full-width (including label column) after restoring the clip
  ctx.fillStyle = '#383838';
  for (const row of newRowLayout) {
    ctx.fillRect(0, row.top + row.height - 2, canvasW, 2);
  }

  if (props.showAutomation) {
    ctx.fillStyle = '#222';
    ctx.fillRect(0, masterTopY + MASTER_ROW_H - 1, canvasW, 1);
  }

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

  drawOverview(ctx, canvasW, trackW, overviewY, totalMs, viewStart, viewEnd);

  // Separators framing the track area (left label column, right padding column)
  ctx.fillStyle = '#2a2a2a';
  ctx.fillRect(LABEL_W - 1, 0, 1, canvasH);
  ctx.fillRect(canvasW - PADDING, 0, 1, canvasH);
}

function drawOverview(
  ctx: CanvasRenderingContext2D,
  canvasW: number,
  trackW: number,
  stripY: number,
  totalMs: number,
  viewStart: number,
  viewEnd: number
) {
  overviewRect = { y: stripY, h: OVERVIEW_H };

  function fullMsToX(ms: number) {
    return LABEL_W + (ms / totalMs) * trackW;
  }

  ctx.fillStyle = '#0a0a0a';
  ctx.fillRect(0, stripY, canvasW, OVERVIEW_H);

  const laneH = (OVERVIEW_H - 4) / DECK_ORDER.length;
  for (let ri = 0; ri < DECK_ORDER.length; ri++) {
    const deckId = DECK_ORDER[ri];
    const accent = DECK_ACCENT[deckId];
    const overviewLaneY = stripY + 2 + ri * laneH;
    ctx.fillStyle = accent + 'aa';
    for (const clip of props.clips) {
      if (clip.deck !== deckId) continue;
      const clipX = fullMsToX(clip.sessionStartMs);
      const clipW = Math.max(1, fullMsToX(clip.sessionEndMs) - clipX);
      ctx.fillRect(clipX, overviewLaneY, clipW, Math.max(1, laneH - 1));
    }
  }

  if (props.playheadMs > 0) {
    const playheadX = fullMsToX(props.playheadMs);
    ctx.fillStyle = '#ffffffcc';
    ctx.fillRect(playheadX - 0.5, stripY, 1, OVERVIEW_H);
  }

  const viewportX = fullMsToX(viewStart);
  const viewportW = Math.max(2, fullMsToX(viewEnd) - viewportX);
  ctx.fillStyle = '#ffffff15';
  ctx.fillRect(viewportX, stripY, viewportW, OVERVIEW_H);
  ctx.strokeStyle = '#ffffff80';
  ctx.lineWidth = 1;
  ctx.strokeRect(viewportX + 0.5, stripY + 0.5, Math.max(1, viewportW - 1), OVERVIEW_H - 1);

  ctx.fillStyle = '#222';
  ctx.fillRect(0, stripY - 1, canvasW, 1);
}

function formatTickLabel(ms: number, tickIntervalMs: number): string {
  if (tickIntervalMs < 1000) {
    const totalSec = Math.floor(ms / 1000);
    const mins = Math.floor(totalSec / 60);
    const secs = totalSec % 60;
    const millis = ms % 1000;
    return `${mins}:${String(secs).padStart(2, '0')}:${String(millis).padStart(3, '0')}`;
  }
  return formatMs(ms);
}

function valueToY(laneY: number, laneH: number, minVal: number, maxVal: number, value: number) {
  const range = maxVal - minVal || 1;
  return laneY + laneH - ((value - minVal) / range) * laneH;
}

// Draws a zero-order-hold (step) graph: each point's value is held flat until
// the next point's time, then jumps. This matches how these parameters actually
// change, recorded session events fire at the moment a value changes, not gradually
// beforehand, so a held-then-jump shape is accurate while a straight diagonal is not.
function drawAutomationSteps(
  ctx: CanvasRenderingContext2D,
  points: AutomationPoint[],
  laneY: number,
  laneH: number,
  minVal: number,
  maxVal: number,
  color: string | ((value: number) => string),
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
) {
  const visible = sliceVisiblePoints(points, viewStart, viewEnd);
  if (visible.length < 2) return;
  const colorFor = typeof color === 'function' ? color : () => color;
  ctx.lineWidth = 1;
  for (let pointIdx = 0; pointIdx < visible.length - 1; pointIdx++) {
    const cur = visible[pointIdx];
    const next = visible[pointIdx + 1];
    const segX0 = msToX(cur.ms);
    const segX1 = msToX(next.ms);
    const stepY = valueToY(laneY, laneH, minVal, maxVal, cur.value);
    const nextStepY = valueToY(laneY, laneH, minVal, maxVal, next.value);

    ctx.strokeStyle = colorFor(cur.value);
    ctx.beginPath();
    ctx.moveTo(segX0, stepY);
    ctx.lineTo(segX1, stepY);
    ctx.stroke();

    if (nextStepY !== stepY) {
      ctx.strokeStyle = colorFor(next.value);
      ctx.beginPath();
      ctx.moveTo(segX1, stepY);
      ctx.lineTo(segX1, nextStepY);
      ctx.stroke();
    }
  }
}

function filterColorFor(value: number): string {
  if (value < -FILTER_DEAD_ZONE) return FILTER_LPF_COLOR;
  if (value > FILTER_DEAD_ZONE) return FILTER_HPF_COLOR;
  return FILTER_NEUTRAL_COLOR;
}

function drawFilterLane(
  ctx: CanvasRenderingContext2D,
  w: number,
  automation: DeckAutomation,
  laneY: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
) {
  for (const span of automation.filterActive) {
    if (!overlapsRange(span.startMs, span.endMs, viewStart, viewEnd)) continue;
    const sx = msToX(span.startMs);
    const sw = Math.max(1, msToX(span.endMs) - sx);
    ctx.fillStyle = FILTER_ACTIVE_FILL;
    ctx.fillRect(sx, laneY, sw, SUBLANE_H);
  }

  // Center line marks the bypass position (knob = 0); LPF sweeps below it, HPF above.
  const centerY = valueToY(laneY, SUBLANE_H, -1, 1, 0);
  ctx.strokeStyle = '#3a3a3a';
  ctx.beginPath();
  ctx.moveTo(LABEL_W, centerY);
  ctx.lineTo(w - PADDING, centerY);
  ctx.stroke();

  drawAutomationSteps(
    ctx,
    automation.filter,
    laneY,
    SUBLANE_H,
    -1,
    1,
    filterColorFor,
    msToX,
    viewStart,
    viewEnd
  );
}

function drawEqLane(
  ctx: CanvasRenderingContext2D,
  points: AutomationPoint[],
  color: string,
  laneY: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
) {
  drawAutomationSteps(
    ctx,
    points,
    laneY,
    SUBLANE_H,
    EQ_MIN_DB,
    EQ_MAX_DB,
    color,
    msToX,
    viewStart,
    viewEnd
  );
}

type LaneDrawer = (
  ctx: CanvasRenderingContext2D,
  w: number,
  automation: DeckAutomation,
  laneY: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
) => void;

const LANE_DRAWERS: Record<LaneKey, LaneDrawer> = {
  gain: (ctx, _w, automation, laneY, msToX, viewStart, viewEnd) =>
    drawAutomationSteps(
      ctx,
      automation.gain,
      laneY,
      SUBLANE_H,
      0,
      1,
      GAIN_COLOR,
      msToX,
      viewStart,
      viewEnd
    ),
  filter: drawFilterLane,
  eqLow: (ctx, _w, automation, laneY, msToX, viewStart, viewEnd) =>
    drawEqLane(ctx, automation.eqLow, EQ_BAND_COLORS.low, laneY, msToX, viewStart, viewEnd),
  eqMid: (ctx, _w, automation, laneY, msToX, viewStart, viewEnd) =>
    drawEqLane(ctx, automation.eqMid, EQ_BAND_COLORS.mid, laneY, msToX, viewStart, viewEnd),
  eqHigh: (ctx, _w, automation, laneY, msToX, viewStart, viewEnd) =>
    drawEqLane(ctx, automation.eqHigh, EQ_BAND_COLORS.high, laneY, msToX, viewStart, viewEnd)
};

function drawDeckAutomation(
  ctx: CanvasRenderingContext2D,
  w: number,
  laneTopY: number,
  msToX: (ms: number) => number,
  automation: DeckAutomation | undefined,
  lanes: LaneKey[],
  viewStart: number,
  viewEnd: number
) {
  if (!automation) return;

  let laneY = laneTopY;
  for (let laneIdx = 0; laneIdx < lanes.length; laneIdx++) {
    const group = LANE_GROUP[lanes[laneIdx]];
    const prevGroup = laneIdx > 0 ? LANE_GROUP[lanes[laneIdx - 1]] : -1;
    const groupChanged = group !== prevGroup;

    ctx.fillStyle = group % 2 === 0 ? '#1a1a1a' : '#141414';
    ctx.fillRect(LABEL_W, laneY, w - LABEL_W - PADDING, SUBLANE_H);

    if (laneIdx > 0 && groupChanged) {
      ctx.fillStyle = '#2a2a2a';
      ctx.fillRect(LABEL_W, laneY, w - LABEL_W - PADDING, 1);
    }

    LANE_DRAWERS[lanes[laneIdx]](ctx, w, automation, laneY, msToX, viewStart, viewEnd);
    laneY += SUBLANE_H;
  }
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
const FOLLOW_LEAD_IN_FRACTION = 0.1;

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
    props.showAutomation,
    props.deckAutomation,
    props.masterAutomation,
    props.deckNudges,
    laneVisibility.value,
    viewStartMs.value,
    viewDurationMs.value
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
