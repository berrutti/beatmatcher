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
import { spectralColor } from '@renderer/utils/waveformImage';
import { beatLineStep, beatTier } from '@renderer/utils/beatGrid';
import { loopRegionRect, drawLoopRegionOverlay } from '@renderer/utils/loopRegionRect';
import { computeCanvasSize } from '@renderer/utils/canvasResize';

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
  accent: string;
};

const props = defineProps<{ sources: WaveformStripsSource[] }>();
const emit = defineEmits<{
  'scrub-start': [sourceIndex: number];
  scrub: [sourceIndex: number, sec: number];
  'scrub-end': [sourceIndex: number];
}>();

const HALF_WINDOW_SEC = 5;
const OFFSCREEN_CROSS = 256;
const MIN_BEAT_LINE_SPACING_PX = 6;
const BEATS_PER_BAR = 4;
const BEATS_PER_PHRASE = 16;
const BEAT_LINE_ALPHA = 0.35;
// Device pixels, not CSS lineWidth, since these are fillRect not stroke.
const BEAT_LINE_DEVICE_WIDTH = 2;
const BAR_LINE_OUTLINE_DEVICE_WIDTH = 6;
const BAR_LINE_CORE_DEVICE_WIDTH = 2;
const BAR_LINE_OUTLINE_COLOR = 'rgba(0,0,0,0.9)';
const BAR_LINE_CORE_COLOR = '#ffffff';
const BAR_MARKER_TRI_W = 5;
const BAR_MARKER_TRI_H = 6;
const BAR_MARKER_FILL_COLOR = '#ffffff';
const BAR_MARKER_OUTLINE_COLOR = '#000000';
const BAR_MARKER_OUTLINE_WIDTH = 1.5;
const EMPTY_STRIP_BG = '#141414';
const STRIP_SEPARATOR_COLOR = '#2a2a2a';
// Pre-render ±30s around the playhead. At 250 pts/sec this caps the offscreen
// at 15,000 columns, well within WebKit's ~32k canvas dimension limit.
const BUFFER_SEC = 30;

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
  // Aggregated steps/sec stored in the offscreen. Chosen so that 1 step ≈ 1
  // physical pixel in the visible window, eliminating downscale aliasing.
  displayRate: number;
  numSteps: number;
  bufferStartSec: number;
  lastBuiltMain: number;
  lastBuiltDpr: number;
  isBuilding: boolean;
};

let states: OffscreenState[] = [];

function initStates() {
  states = props.sources.map(() => ({
    canvas: null,
    builtFrom: null,
    denseRate: 0,
    displayRate: 0,
    numSteps: 0,
    bufferStartSec: 0,
    lastBuiltMain: 0,
    lastBuiltDpr: 0,
    isBuilding: false
  }));
}

async function buildOffscreenWindow(i: number, centerPos: number, mainSize: number, dpr: number) {
  const state = states[i];
  state.isBuilding = true;
  await new Promise<void>((r) => setTimeout(r, 0));

  const src = props.sources[i];
  const data = src.getDenseData();

  if (!data) {
    state.canvas = null;
    state.builtFrom = null;
    state.isBuilding = false;
    return;
  }

  const denseRate = src.getDenseRate();
  const totalSamples = Math.floor(data.length / 4);
  const totalDuration = totalSamples / denseRate;

  // Target: 1 aggregated step per physical pixel in the 10-second visible window
  const physicalMain = mainSize * dpr;
  const targetDisplayRate = physicalMain / (2 * HALF_WINDOW_SEC);
  const stride = Math.max(1, Math.round(denseRate / targetDisplayRate));
  const displayRate = denseRate / stride;

  const bufferStartSec = Math.max(0, centerPos - BUFFER_SEC);
  const bufferEndSec = Math.min(totalDuration, centerPos + BUFFER_SEC);
  const startSample = Math.floor(bufferStartSec * denseRate);
  const endSample = Math.min(totalSamples, Math.ceil(bufferEndSec * denseRate));
  const numSteps = Math.ceil((endSample - startSample) / stride);

  const canvas = document.createElement('canvas');
  canvas.width = numSteps;
  canvas.height = OFFSCREEN_CROSS;
  const ctx = canvas.getContext('2d');
  if (!ctx) {
    state.isBuilding = false;
    return;
  }

  const imageData = ctx.createImageData(numSteps, OFFSCREEN_CROSS);
  const px = imageData.data;
  const cy = OFFSCREEN_CROSS / 2;

  for (let col = 0; col < numSteps; col++) {
    let bass = 0,
      mid = 0,
      high = 0,
      amp = 0,
      count = 0;
    for (let k = 0; k < stride; k++) {
      const sampleIdx = startSample + col * stride + k;
      if (sampleIdx >= totalSamples) break;
      const di = sampleIdx * 4;
      bass += data[di];
      mid += data[di + 1];
      high += data[di + 2];
      amp += data[di + 3];
      count++;
    }

    if (count === 0) continue;
    bass /= count;
    mid /= count;
    high /= count;
    amp /= count;

    const [r, g, b] = spectralColor(bass, mid, high);
    const barH = Math.sqrt(amp) * cy * 1.5;
    const yTop = Math.max(0, Math.round(cy - barH));
    const yBottom = Math.min(OFFSCREEN_CROSS - 1, Math.round(cy + barH));

    for (let y = yTop; y <= yBottom; y++) {
      const idx = (y * numSteps + col) * 4;
      px[idx] = r;
      px[idx + 1] = g;
      px[idx + 2] = b;
      px[idx + 3] = 255;
    }

    if ((col + 1) % STEPS_PER_CHUNK === 0) {
      if (src.getDenseData() !== data) {
        state.isBuilding = false;
        return;
      }
      await new Promise<void>((r) => setTimeout(r, 0));
    }
  }

  if (src.getDenseData() !== data) {
    state.isBuilding = false;
    return;
  }

  ctx.putImageData(imageData, 0, 0);

  state.canvas = canvas;
  state.builtFrom = data;
  state.denseRate = denseRate;
  state.displayRate = displayRate;
  state.numSteps = numSteps;
  state.bufferStartSec = bufferStartSec;
  state.lastBuiltMain = mainSize;
  state.lastBuiltDpr = dpr;
  state.isBuilding = false;
}

function stripXFor(width: number, pos: number, rate: number, sec: number): number {
  return width / 2 + (((sec - pos) / rate) * width) / (2 * HALF_WINDOW_SEC);
}

function snapToDevicePixel(x: number, dpr: number): number {
  return Math.round(x * dpr) / dpr;
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

// A stroke's anti-aliased edge would shimmer as position scrolls; a pixel-aligned fill can't.
function fillPixelLine(
  ctx: CanvasRenderingContext2D,
  centerX: number,
  y0: number,
  height: number,
  devicePxWidth: number,
  dpr: number,
  color: string
): void {
  const leftDevicePx = Math.round(centerX * dpr) - devicePxWidth / 2;
  ctx.fillStyle = color;
  ctx.fillRect(leftDevicePx / dpr, y0, devicePxWidth / dpr, height);
}

function drawPlainBeatLine(
  ctx: CanvasRenderingContext2D,
  x: number,
  y0: number,
  stripH: number,
  dpr: number
): void {
  const color = `rgba(255,255,255,${BEAT_LINE_ALPHA})`;
  fillPixelLine(ctx, x, y0, stripH, BEAT_LINE_DEVICE_WIDTH, dpr, color);
}

function drawBarLine(
  ctx: CanvasRenderingContext2D,
  x: number,
  y0: number,
  stripH: number,
  dpr: number
): void {
  fillPixelLine(ctx, x, y0, stripH, BAR_LINE_OUTLINE_DEVICE_WIDTH, dpr, BAR_LINE_OUTLINE_COLOR);
  fillPixelLine(ctx, x, y0, stripH, BAR_LINE_CORE_DEVICE_WIDTH, dpr, BAR_LINE_CORE_COLOR);
}

function drawTriangle(
  ctx: CanvasRenderingContext2D,
  x: number,
  yBase: number,
  pointHeight: number
): void {
  ctx.beginPath();
  ctx.moveTo(x - BAR_MARKER_TRI_W, yBase);
  ctx.lineTo(x + BAR_MARKER_TRI_W, yBase);
  ctx.lineTo(x, yBase + pointHeight);
  ctx.closePath();
  ctx.fillStyle = BAR_MARKER_FILL_COLOR;
  ctx.strokeStyle = BAR_MARKER_OUTLINE_COLOR;
  ctx.lineWidth = BAR_MARKER_OUTLINE_WIDTH;
  ctx.stroke();
  ctx.fill();
}

// Two triangles so a bar reads at a glance instead of only through line density.
function drawBarMarker(ctx: CanvasRenderingContext2D, x: number, y0: number, stripH: number): void {
  ctx.save();
  drawTriangle(ctx, x, y0, BAR_MARKER_TRI_H);
  drawTriangle(ctx, x, y0 + stripH, -BAR_MARKER_TRI_H);
  ctx.restore();
}

// LOD-stepped so lines stay legibly spaced instead of overlapping into noise
// at high BPM or when zoomed out.
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
  const beatPeriod = 60 / bpm;
  const audioHalfWindow = HALF_WINDOW_SEC * rate;
  const pxPerBeat = (beatPeriod / rate) * (width / (2 * HALF_WINDOW_SEC));
  const step = beatLineStep(pxPerBeat, MIN_BEAT_LINE_SPACING_PX, BEATS_PER_BAR);

  const nStart = Math.ceil((pos - audioHalfWindow - beatOffset) / beatPeriod);
  const nEnd = Math.floor((pos + audioHalfWindow - beatOffset) / beatPeriod);

  for (let bn = nStart; bn <= nEnd; bn++) {
    if (bn % step !== 0) continue;
    const tBeat = beatOffset + bn * beatPeriod;
    const xBeat = xFor(tBeat);

    if (beatTier(bn, BEATS_PER_BAR, BEATS_PER_PHRASE) === 'beat') {
      drawPlainBeatLine(ctx, xBeat, y0, stripH, dpr);
    } else {
      drawBarLine(ctx, xBeat, y0, stripH, dpr);
      drawBarMarker(ctx, xBeat, y0, stripH);
    }
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
  // Catches DPR changes too, not just CSS size (ResizeObserver misses those); no-ops if unchanged.
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
      pos < state.bufferStartSec + edgeGuard ||
      pos > bufferEndSec - edgeGuard;

    if (needsRebuild && !state.isBuilding) buildOffscreenWindow(i, pos, w, dpr).catch(() => {});
    if (!state.canvas) {
      drawEmptyStrip(ctx, y0, w, stripH);
      continue;
    }

    // rate > 1 means pitched up: audio advances faster than real time, so the
    // waveform appears compressed horizontally (fewer audio seconds fit in the
    // fixed real-time window). Divide all audio-time offsets by rate to convert
    // to real-time screen coordinates.
    const rate = Math.max(0.1, src.getRate());
    const scaleX = w / (2 * HALF_WINDOW_SEC * state.displayRate * rate);
    // Snap to physical pixel boundary to eliminate sub-pixel shimmer
    const txRaw = w / 2 - (pos - state.bufferStartSec) * state.displayRate * scaleX;
    const tx = Math.round(txRaw * dpr) / dpr;

    drawStripWaveform(ctx, state.canvas, state.numSteps, y0, w, stripH, tx, scaleX);

    const xFor = (sec: number) => stripXFor(w, pos, rate, sec);

    const loopRegion = src.getLoopRegion();
    if (loopRegion) drawLoopRegion(ctx, y0, w, stripH, loopRegion, src.getLoopActive(), xFor);

    const bpm = src.getBpm();
    if (bpm !== null && bpm > 0) {
      drawBeatGrid(ctx, y0, w, stripH, bpm, src.getBeatOffset(), rate, pos, xFor, dpr);
    }

    if (src.isWaveformLoading()) drawLoadingOverlay(ctx, y0, w, stripH);
  }

  // x is the same for every strip, unlike the per-strip draws in the loop above.
  drawPlayhead(ctx, snapToDevicePixel(w / 2, dpr), h);
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
  // Force offscreen rebuild at new size
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
    initStates();
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
