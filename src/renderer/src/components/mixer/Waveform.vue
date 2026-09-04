<template>
  <div class="strips-wrapper">
    <canvas
      ref="canvasEl"
      class="strips-canvas"
      :class="{ 'strips-canvas--dragging': drag !== null }"
      :style="{ cursor: canvasCursor }"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointercancel="onPointerUp"
      @mousemove="onMouseMove"
      @mouseleave="hoveredStripIndex = -1"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import {
  waveformColumns,
  paintWaveformColumns,
  type WaveformPaint
} from '@renderer/utils/waveformImage';
import { beatGridStep, visibleBeats } from '@renderer/utils/beatGrid';
import { loopRegionRect, drawLoopRegionOverlay } from '@renderer/utils/loopRegionRect';
import { computeCanvasSize } from '@renderer/utils/canvasResize';
import { CUE_CHANNELS, drawCueTriangle } from '@renderer/utils/cueMarker';
import {
  MARKER_OUTLINE_COLOR,
  MIN_WAVEFORM_BEAT_SPACING_PX,
  MIN_WAVEFORM_BAR_SPACING_PX,
  STRIP_GRID,
  fillPixelLine,
  drawBeatLine,
  drawBarLine,
  drawBeatMarker
} from '@renderer/utils/beatGridDraw';
import {
  stripColumnRate,
  stripScaleX,
  snappedToDevicePixel,
  stripX
} from '@renderer/utils/stripGeometry';
import { waveformPalette } from '@renderer/utils/waveformPalettes';
import { STRIP_SCALES } from '@renderer/utils/waveformPaints';
import type { WaveformStyleOption } from '@renderer/utils/types';

type WaveformStripsSource = {
  getPosition: () => number;
  getBpm: () => number | null;
  getBeatOffset: () => number;
  getRate: () => number;
  getDenseData: () => Float32Array | null;
  getDenseRate: () => number;
  isWaveformLoading: () => boolean;
  getLoopRegion: () => { startSec: number; endSec: number } | null;
  getLoopActive: () => boolean;
  getCuePoint: () => number;
  getBandBalance: () => [number, number, number];
};

const props = defineProps<{
  sources: WaveformStripsSource[];
  waveformStyle: WaveformStyleOption;
}>();
const emit = defineEmits<{
  'scrub-start': [sourceIndex: number];
  scrub: [sourceIndex: number, sec: number];
  'scrub-end': [sourceIndex: number];
}>();

const HALF_WINDOW_SEC = 5;
const OFFSCREEN_CROSS = 256;
const BEATS_PER_BAR = 4;
function stripPaint(balance: [number, number, number]): WaveformPaint {
  return { ...STRIP_SCALES, palette: waveformPalette(props.waveformStyle, balance) };
}
const CUE_TRI_W = 6;
const CUE_TRI_H = 9;
const CUE_LINE_DEVICE_WIDTH = 2;
const CUE_LINE_ALPHA = 0.5;
const EMPTY_STRIP_BG = '#141414';
const STRIP_SEPARATOR_COLOR = '#2a2a2a';
// Pre-render ±30s around the playhead, shortened on a wide strip so the offscreen stays
// under WebKit's ~32k canvas dimension limit.
const BUFFER_SEC = 30;
const MAX_OFFSCREEN_COLUMNS = 30000;

const canvasEl = ref<HTMLCanvasElement | null>(null);
let rafId = 0;
let resizeObserver: ResizeObserver | null = null;

type DragState = { stripIndex: number; anchorX: number; anchorPos: number };
const drag = ref<DragState | null>(null);
const hoveredStripIndex = ref(-1);

const canvasCursor = computed(() => {
  if (drag.value !== null) return 'grabbing';
  if (hoveredStripIndex.value >= 0 && states[hoveredStripIndex.value]?.canvas !== null)
    return 'grab';
  return 'default';
});

function onPointerDown(e: PointerEvent) {
  const canvas = canvasEl.value;
  if (!canvas) return;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (!w || !h) return;
  const rect = canvas.getBoundingClientRect();
  const y = e.clientY - rect.top;
  const n = props.sources.length;
  const stripH = h / n;
  const stripIndex = Math.min(Math.floor(y / stripH), n - 1);
  if (states[stripIndex]?.canvas === null) return;
  drag.value = {
    stripIndex,
    anchorX: e.clientX,
    anchorPos: props.sources[stripIndex].getPosition()
  };
  canvas.setPointerCapture(e.pointerId);
  emit('scrub-start', stripIndex);
}

function onPointerMove(e: PointerEvent) {
  if (!drag.value) return;
  const canvas = canvasEl.value;
  if (!canvas) return;
  const w = canvas.clientWidth;
  if (!w) return;
  const src = props.sources[drag.value.stripIndex];
  const rate = Math.max(0.1, src.getRate());
  const dx = e.clientX - drag.value.anchorX;
  const sec = Math.max(0, drag.value.anchorPos - (dx * (2 * HALF_WINDOW_SEC * rate)) / w);
  emit('scrub', drag.value.stripIndex, sec);
}

function onPointerUp() {
  if (drag.value) emit('scrub-end', drag.value.stripIndex);
  drag.value = null;
}

function onMouseMove(e: MouseEvent) {
  const canvas = canvasEl.value;
  if (drag.value !== null || !canvas || !canvas.clientHeight) return;
  const rect = canvas.getBoundingClientRect();
  const y = e.clientY - rect.top;
  const n = props.sources.length;
  hoveredStripIndex.value = Math.min(Math.floor((y / canvas.clientHeight) * n), n - 1);
}

const STEPS_PER_CHUNK = 500;

type OffscreenState = {
  canvas: HTMLCanvasElement | null;
  builtFrom: Float32Array | null;
  denseRate: number;
  displayRate: number;
  numSteps: number;
  bufferStartSec: number;
  lastBuiltMain: number;
  lastBuiltDpr: number;
  lastBuiltStyle: WaveformStyleOption | null;
};

const EMPTY_STATE: OffscreenState = {
  canvas: null,
  builtFrom: null,
  denseRate: 0,
  displayRate: 0,
  numSteps: 0,
  bufferStartSec: 0,
  lastBuiltMain: 0,
  lastBuiltDpr: 0,
  lastBuiltStyle: null
};

let states: OffscreenState[] = [];
let building: boolean[] = [];

function resetStates() {
  states = props.sources.map(() => ({ ...EMPTY_STATE }));
  building = props.sources.map(() => false);
}

// Null keeps whatever the strip already has: the track changed under us mid-build.
async function offscreenWindow(
  i: number,
  centerPos: number,
  mainSize: number,
  dpr: number
): Promise<OffscreenState | null> {
  await new Promise<void>((r) => setTimeout(r, 0));

  const src = props.sources[i];
  const data = src.getDenseData();
  if (!data) return { ...EMPTY_STATE };

  const denseRate = src.getDenseRate();
  const totalPoints = Math.floor(data.length / 4);
  const totalDuration = totalPoints / denseRate;

  const displayRate = stripColumnRate(mainSize * dpr, HALF_WINDOW_SEC);

  const halfBufferSec = Math.min(BUFFER_SEC, MAX_OFFSCREEN_COLUMNS / displayRate / 2);
  const bufferStartSec = Math.max(0, centerPos - halfBufferSec);
  const bufferEndSec = Math.min(totalDuration, centerPos + halfBufferSec);
  const startPoint = Math.floor(bufferStartSec * denseRate);
  const endPoint = Math.min(totalPoints, Math.ceil(bufferEndSec * denseRate));
  const spanSec = (endPoint - startPoint) / denseRate;
  const numSteps = Math.max(1, Math.round(spanSec * displayRate));

  const canvas = document.createElement('canvas');
  canvas.width = numSteps;
  canvas.height = OFFSCREEN_CROSS;
  const ctx = canvas.getContext('2d');
  if (!ctx) return null;

  const style = stripPaint(src.getBandBalance());
  const columns = waveformColumns(data, numSteps, startPoint, endPoint);
  const imageData = ctx.createImageData(numSteps, OFFSCREEN_CROSS);

  // Chunked because painting 15,000 columns of 256 rows in one go drops frames.
  for (let from = 0; from < numSteps; from += STEPS_PER_CHUNK) {
    paintWaveformColumns(
      imageData,
      columns,
      style,
      from,
      Math.min(numSteps, from + STEPS_PER_CHUNK)
    );
    if (src.getDenseData() !== data) return null;
    await new Promise<void>((r) => setTimeout(r, 0));
  }

  if (src.getDenseData() !== data) return null;
  ctx.putImageData(imageData, 0, 0);

  return {
    canvas,
    builtFrom: data,
    denseRate,
    displayRate,
    numSteps,
    bufferStartSec,
    lastBuiltMain: mainSize,
    lastBuiltDpr: dpr,
    lastBuiltStyle: props.waveformStyle
  };
}

async function refreshOffscreen(i: number, centerPos: number, mainSize: number, dpr: number) {
  // resetStates swaps both arrays, and a build can outlive the sources it started on.
  const generation = states;
  building[i] = true;
  try {
    const next = await offscreenWindow(i, centerPos, mainSize, dpr);
    if (next && states === generation) states[i] = next;
  } finally {
    if (states === generation) building[i] = false;
  }
}

function drawEmptyStrip(
  ctx: CanvasRenderingContext2D,
  y0: number,
  width: number,
  stripH: number
): void {
  ctx.fillStyle = EMPTY_STRIP_BG;
  ctx.fillRect(0, y0, width, stripH);
}

function drawStripWaveform(
  ctx: CanvasRenderingContext2D,
  bitmap: HTMLCanvasElement,
  numSteps: number,
  y0: number,
  width: number,
  stripH: number,
  tx: number,
  scaleX: number
): void {
  ctx.save();
  ctx.beginPath();
  ctx.rect(0, y0, width, stripH);
  ctx.clip();
  ctx.drawImage(bitmap, 0, 0, numSteps, OFFSCREEN_CROSS, tx, y0, numSteps * scaleX, stripH);
  ctx.restore();
}

function drawLoopRegion(
  ctx: CanvasRenderingContext2D,
  y0: number,
  width: number,
  stripH: number,
  region: { startSec: number; endSec: number },
  active: boolean,
  xFor: (sec: number) => number
): void {
  const rect = loopRegionRect(xFor, region, width);
  if (!rect) return;
  drawLoopRegionOverlay(ctx, rect, y0, stripH, active);
}

function drawCueMarker(
  ctx: CanvasRenderingContext2D,
  x: number,
  y0: number,
  width: number,
  stripH: number,
  dpr: number
): void {
  if (x < -CUE_TRI_W || x > width + CUE_TRI_W) return;
  const lineColor = `rgba(${CUE_CHANNELS}, ${CUE_LINE_ALPHA})`;
  fillPixelLine(ctx, x, y0, stripH, CUE_LINE_DEVICE_WIDTH, dpr, lineColor);
  drawCueTriangle(ctx, x, y0 + stripH, CUE_TRI_W, -CUE_TRI_H, MARKER_OUTLINE_COLOR);
}

function drawBeatGrid(
  ctx: CanvasRenderingContext2D,
  y0: number,
  width: number,
  stripH: number,
  bpm: number,
  beatOffset: number,
  rate: number,
  pos: number,
  xFor: (sec: number) => number,
  dpr: number
): void {
  const pxPerBeat = (60 / bpm / rate) * (width / (2 * HALF_WINDOW_SEC));
  const step = beatGridStep(
    pxPerBeat,
    BEATS_PER_BAR,
    MIN_WAVEFORM_BEAT_SPACING_PX,
    MIN_WAVEFORM_BAR_SPACING_PX
  );
  const audioHalfWindow = HALF_WINDOW_SEC * rate;

  for (const { sec, isDownbeat } of visibleBeats(
    bpm,
    beatOffset,
    pos - audioHalfWindow,
    pos + audioHalfWindow,
    step,
    BEATS_PER_BAR
  )) {
    const xBeat = xFor(sec);

    if (isDownbeat) {
      drawBarLine(ctx, xBeat, y0, stripH, dpr, STRIP_GRID);
    } else {
      drawBeatLine(ctx, xBeat, y0, stripH, dpr, STRIP_GRID);
    }
    drawBeatMarker(ctx, xBeat, y0, stripH, isDownbeat, STRIP_GRID);
  }
}

function drawLoadingOverlay(
  ctx: CanvasRenderingContext2D,
  y0: number,
  width: number,
  stripH: number
): void {
  ctx.fillStyle = 'rgba(0,0,0,0.6)';
  ctx.fillRect(0, y0, width, stripH);
}

function drawPlayhead(ctx: CanvasRenderingContext2D, x: number, height: number): void {
  ctx.lineWidth = 3;
  ctx.strokeStyle = 'rgba(0,0,0,0.9)';
  ctx.beginPath();
  ctx.moveTo(x, 0);
  ctx.lineTo(x, height);
  ctx.stroke();
  ctx.lineWidth = 1;
  ctx.strokeStyle = 'rgba(220,30,30,1)';
  ctx.beginPath();
  ctx.moveTo(x, 0);
  ctx.lineTo(x, height);
  ctx.stroke();
}

function drawStripSeparators(
  ctx: CanvasRenderingContext2D,
  stripCount: number,
  width: number,
  stripH: number
): void {
  for (let i = 1; i < stripCount; i++) {
    ctx.fillStyle = STRIP_SEPARATOR_COLOR;
    ctx.fillRect(0, i * stripH, width, 1);
  }
}

function draw() {
  const canvas = canvasEl.value;
  if (!canvas) return;
  // Catches DPR changes too, not just CSS size (ResizeObserver misses those). No-ops if unchanged.
  resizeCanvas(canvas);
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.width / dpr;
  const h = canvas.height / dpr;

  if (!w || !h) {
    rafId = requestAnimationFrame(draw);
    return;
  }

  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = 'high';

  ctx.clearRect(0, 0, w, h);

  const n = props.sources.length;
  const stripH = Math.floor(h / n);

  for (let i = 0; i < n; i++) {
    const src = props.sources[i];
    const state = states[i];
    const y0 = i * stripH;

    const pos = src.getPosition();
    const bufferEndSec = state.bufferStartSec + state.numSteps / (state.displayRate || 1);
    const edgeGuard = HALF_WINDOW_SEC + 5;
    const needsRebuild =
      state.builtFrom !== src.getDenseData() ||
      state.lastBuiltMain !== w ||
      state.lastBuiltDpr !== dpr ||
      state.lastBuiltStyle !== props.waveformStyle ||
      pos < state.bufferStartSec + edgeGuard ||
      pos > bufferEndSec - edgeGuard;

    if (needsRebuild && !building[i]) refreshOffscreen(i, pos, w, dpr).catch(() => {});
    if (!state.canvas) {
      drawEmptyStrip(ctx, y0, w, stripH);
      continue;
    }

    // Audio-time offsets divide by rate to reach screen coordinates: pitched up,
    // fewer audio seconds fit the fixed real-time window.
    const rate = Math.max(0.1, src.getRate());
    const scaleX = stripScaleX(w, HALF_WINDOW_SEC, state.displayRate, rate);
    const txRaw = w / 2 - (pos - state.bufferStartSec) * state.displayRate * scaleX;
    const tx = snappedToDevicePixel(txRaw, dpr);

    drawStripWaveform(ctx, state.canvas, state.numSteps, y0, w, stripH, tx, scaleX);

    const xFor = (sec: number) => stripX(w, HALF_WINDOW_SEC, pos, rate, sec);

    const loopRegion = src.getLoopRegion();
    if (loopRegion) drawLoopRegion(ctx, y0, w, stripH, loopRegion, src.getLoopActive(), xFor);

    const bpm = src.getBpm();
    if (bpm !== null && bpm > 0) {
      drawBeatGrid(ctx, y0, w, stripH, bpm, src.getBeatOffset(), rate, pos, xFor, dpr);
    }

    drawCueMarker(ctx, xFor(src.getCuePoint()), y0, w, stripH, dpr);

    if (src.isWaveformLoading()) drawLoadingOverlay(ctx, y0, w, stripH);
  }

  // x is the same for every strip, unlike the per-strip draws in the loop above.
  drawPlayhead(ctx, snappedToDevicePixel(w / 2, dpr), h);
  drawStripSeparators(ctx, n, w, stripH);

  rafId = requestAnimationFrame(draw);
}

function resizeCanvas(canvas: HTMLCanvasElement) {
  const dpr = window.devicePixelRatio || 1;
  const target = computeCanvasSize(canvas.clientWidth, canvas.clientHeight, dpr);
  if (!target) return;
  if (canvas.width === target.width && canvas.height === target.height) return;
  canvas.width = target.width;
  canvas.height = target.height;
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.scale(dpr, dpr);
  for (const s of states) {
    s.lastBuiltMain = 0;
    s.lastBuiltDpr = 0;
  }
}

onMounted(() => {
  const canvas = canvasEl.value;
  if (!canvas) return;
  requestAnimationFrame(() => {
    resizeCanvas(canvas);
    resetStates();
    rafId = requestAnimationFrame(draw);
  });
  resizeObserver = new ResizeObserver(() => {
    requestAnimationFrame(() => resizeCanvas(canvas));
  });
  resizeObserver.observe(canvas);
});

onUnmounted(() => {
  cancelAnimationFrame(rafId);
  resizeObserver?.disconnect();
});
</script>

<style scoped>
.strips-wrapper {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  width: 100%;
  height: 100%;
}

.strips-canvas {
  display: block;
  width: 100%;
  flex: 1;
  min-height: 0;
  height: 100%;
  background: var(--color-bg);
}
</style>
