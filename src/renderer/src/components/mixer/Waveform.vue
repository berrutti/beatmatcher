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

type WaveformStripsSource = {
  getPosition: () => number;
  getBpm: () => number | null;
  getBeatOffset: () => number;
  getRate: () => number;
  getDenseData: () => Float32Array | null;
  getDenseRate: () => number;
  isWaveformLoading: () => boolean;
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

function draw() {
  const canvas = canvasEl.value;
  if (!canvas) return;
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
    if (!state.canvas) continue;

    // rate > 1 means pitched up: audio advances faster than real time, so the
    // waveform appears compressed horizontally (fewer audio seconds fit in the
    // fixed real-time window). Divide all audio-time offsets by rate to convert
    // to real-time screen coordinates.
    const rate = Math.max(0.1, src.getRate());
    const scaleX = w / (2 * HALF_WINDOW_SEC * state.displayRate * rate);
    // Snap to physical pixel boundary to eliminate sub-pixel shimmer
    const txRaw = w / 2 - (pos - state.bufferStartSec) * state.displayRate * scaleX;
    const tx = Math.round(txRaw * dpr) / dpr;

    ctx.save();
    ctx.beginPath();
    ctx.rect(0, y0, w, stripH);
    ctx.clip();
    ctx.drawImage(
      state.canvas,
      0,
      0,
      state.numSteps,
      OFFSCREEN_CROSS,
      tx,
      y0,
      state.numSteps * scaleX,
      stripH
    );
    ctx.restore();

    // Beat grid markers. tBeat is in audio time; divide by rate to get real-time offset
    const bpm = src.getBpm();
    if (bpm !== null) {
      const beatOffset = src.getBeatOffset();
      const beatPeriod = 60 / bpm;
      const audioHalfWindow = HALF_WINDOW_SEC * rate;
      const nStart = Math.ceil((pos - audioHalfWindow - beatOffset) / beatPeriod);
      const nEnd = Math.floor((pos + audioHalfWindow - beatOffset) / beatPeriod);

      for (let bn = nStart; bn <= nEnd; bn++) {
        const tBeat = beatOffset + bn * beatPeriod;
        const xBeat = w / 2 + (((tBeat - pos) / rate) * w) / (2 * HALF_WINDOW_SEC);
        const alpha = bn % 4 === 0 ? 0.8 : 0.4;
        ctx.lineWidth = 3;
        ctx.strokeStyle = `rgba(0,0,0,${alpha})`;
        ctx.beginPath();
        ctx.moveTo(xBeat, y0);
        ctx.lineTo(xBeat, y0 + stripH);
        ctx.stroke();
        ctx.lineWidth = 1;
        ctx.strokeStyle = `rgba(255,255,255,${alpha})`;
        ctx.beginPath();
        ctx.moveTo(xBeat, y0);
        ctx.lineTo(xBeat, y0 + stripH);
        ctx.stroke();
      }
    }

    // Vertical playhead. Same x across all strips; alignment = sync
    ctx.lineWidth = 3;
    ctx.strokeStyle = 'rgba(0,0,0,0.9)';
    ctx.beginPath();
    ctx.moveTo(w / 2, y0);
    ctx.lineTo(w / 2, y0 + stripH);
    ctx.stroke();
    ctx.lineWidth = 1;
    ctx.strokeStyle = 'rgba(220,30,30,1)';
    ctx.beginPath();
    ctx.moveTo(w / 2, y0);
    ctx.lineTo(w / 2, y0 + stripH);
    ctx.stroke();

    if (src.isWaveformLoading()) {
      ctx.fillStyle = 'rgba(0,0,0,0.6)';
      ctx.fillRect(0, y0, w, stripH);
    }
  }

  // Separators: drawn on top so waveform never covers them.
  // Show whenever at least one adjacent strip is loaded; skip only between two empty strips.
  for (let i = 1; i < n; i++) {
    if (states[i - 1].canvas !== null || states[i].canvas !== null) {
      ctx.fillStyle = '#2a2a2a';
      ctx.fillRect(0, i * stripH, w, 1);
    }
  }

  rafId = requestAnimationFrame(draw);
}

function resizeCanvas(canvas: HTMLCanvasElement) {
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (!w || !h) return;
  const dpr = window.devicePixelRatio || 1;
  if (canvas.width === w * dpr && canvas.height === h * dpr) return;
  canvas.width = w * dpr;
  canvas.height = h * dpr;
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
