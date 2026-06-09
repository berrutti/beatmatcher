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
  type OverviewRect,
  formatTickLabel,
  drawLoadedSpan,
  drawLoadedSpanLabel,
  drawClip,
  drawNudgeSpans,
  drawDeckLanes,
  drawMasterGainLane,
  drawOverview,
  makeMsToX
} from '@renderer/utils/timelineDraw';
import { DECK_ACCENTS, DeckId, useDecksStore } from '@renderer/stores/decks';

type DragMode = 'track' | 'overview-move' | 'overview-resize-left' | 'overview-resize-right' | null;
type DragState = { mode: DragMode; startClientX: number; startView: ViewWindow; dragged: boolean };
type LaneMenu = { deck: string; x: number; y: number };

const MIN_VIEW_MS = 200;
const ZOOM_SENSITIVITY = 0.0015;
const DRAG_THRESHOLD_PX = 3;
const EDGE_GRAB_PX = 6;
const FOLLOW_LEAD_IN_FRACTION = 0.1;

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
    const lanes = visibleLanesFor(deckId);
    const rowH = ROW_H + lanes.length * SUBLANE_H;
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
  ctx.fillStyle = '#101010';
  ctx.fillRect(0, masterTopY, canvasW, MASTER_ROW_H);

  ctx.font = `bold 9px monospace`;
  ctx.fillStyle = '#888';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText('M', LABEL_W / 2, masterTopY + MASTER_ROW_H / 2);

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
      drawDeckLanes(
        ctx,
        canvasW,
        rowY + ROW_H,
        msToX,
        props.deckLanes[deckId],
        lanes,
        viewStart,
        viewEnd
      );
    }

    rowY += rowH;
  }
  rowLayout = newRowLayout;

  drawMasterGainLane(ctx, props.masterLanes.gain, masterTopY, msToX, viewStart, viewEnd);
  const bottomY = masterTopY + MASTER_ROW_H;

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

  // Deck row dividers — drawn full-width (including label column) after restoring the clip
  ctx.fillStyle = '#383838';
  for (const row of newRowLayout) {
    ctx.fillRect(0, row.top + row.height - 2, canvasW, 2);
  }

  ctx.fillStyle = '#222';
  ctx.fillRect(0, masterTopY + MASTER_ROW_H - 1, canvasW, 1);

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
