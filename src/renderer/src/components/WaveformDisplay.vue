<template>
  <div class="waveform" :class="{ 'waveform--drag-over': props.isDragOver }">
    <div v-if="!props.trackData" class="waveform__empty">
      <span class="waveform__empty-text">No track loaded</span>
    </div>

    <template v-if="props.trackData">
      <canvas
        ref="canvasEl"
        class="waveform__canvas"
        @mousedown="onMouseDown"
        @wheel.prevent="onWheel"
        @contextmenu.prevent
      />

      <div class="waveform__controls">
        <span class="waveform__bpm-readout" v-if="props.trackBpm">
          {{ props.trackBpm.toFixed(1) }} BPM
        </span>

        <div class="waveform__zoom">
          <button class="waveform__zoom-btn" @click="() => zoomOut()">−</button>
          <span class="waveform__zoom-label">{{ zoomLabel }}</span>
          <button class="waveform__zoom-btn" @click="() => zoomIn()">+</button>
        </div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
// Rendering approach inspired by Mixxx (https://github.com/mixxxdj/mixxx):
// mean-in-window pixel aggregation with an overscan bitmap cache for
// stable, LOD-aware display across all zoom levels.
import { ref, computed, onMounted, onUnmounted, watch } from 'vue';
import type { TrackData } from '@renderer/stores/decks';
import { buildWaveformImageData } from '@renderer/utils/waveformImage';

const props = defineProps<{
  accent: string;
  trackData: TrackData | null;
  isDragOver: boolean;
  trackBpm: number | null;
  beatOffset: number;
  cuePoint: number;
  denseSpectralData: Float32Array | null;
  denseSpectralRate: number;
  getTrackPosition: () => number | null;
  getPlayheadPosition: () => number;
  getSpectralWaveformRegion: (startSec: number, endSec: number, numPoints: number) => Promise<number[]>;
}>();

const emit = defineEmits<{
  setBeatOffset: [sec: number];
  seek: [sec: number];
}>();

const accent = computed(() => props.accent);
const WAVEFORM_AMP_SCALE = 0.9;
const PLAYHEAD_LINE_WIDTH = 1.5;
const PLAYHEAD_ALPHA = 0.9;
const PLAYHEAD_ARROW_HALF = 5;
const PLAYHEAD_ARROW_HEIGHT = 8;

const ZOOM_LEVELS_SEC = [0.25, 0.5, 1, 2, 5, 10, 20, 30, 60, 120, 300];
const DEFAULT_ZOOM_SEC = 10;

const canvasEl = ref<HTMLCanvasElement | null>(null);

let trackDuration = 0;

// Visible window in seconds. Plain vars (not refs) because they're only read
// by the rAF canvas draw loop; reactivity would add per-mousemove overhead
// during drag for no benefit.
let viewStartSec = 0;
let viewEndSec = 0;

const zoomIdx = ref(ZOOM_LEVELS_SEC.indexOf(DEFAULT_ZOOM_SEC));

const zoomLabel = computed(() => {
  const s = ZOOM_LEVELS_SEC[zoomIdx.value];
  return s >= 60 ? `${s / 60}m` : `${s}s`;
});

function viewDurationSec(): number {
  return ZOOM_LEVELS_SEC[zoomIdx.value];
}

function clampView(start: number, duration: number): [number, number] {
  const dur = Math.min(duration, trackDuration);
  let s = Math.max(0, start);
  if (s + dur > trackDuration) s = Math.max(0, trackDuration - dur);
  return [s, s + dur];
}

function setZoomCentered(idx: number, anchorSec?: number) {
  const newZoom = Math.max(0, Math.min(ZOOM_LEVELS_SEC.length - 1, idx));
  if (newZoom === zoomIdx.value) return;
  zoomIdx.value = newZoom;
  const dur = viewDurationSec();
  const center = anchorSec ?? (viewStartSec + viewEndSec) / 2;
  const [s, e] = clampView(center - dur / 2, dur);
  viewStartSec = s;
  viewEndSec = e;
  ensurePeaks();
}

function zoomIn(anchorSec?: number) {
  setZoomCentered(zoomIdx.value - 1, anchorSec);
}
function zoomOut(anchorSec?: number) {
  setZoomCentered(zoomIdx.value + 1, anchorSec);
}

const OVERSCAN_FACTOR = 1.0;
const MAX_BITMAP_PX = 8192;

let cachedPeaks: Float32Array | null = null;
let cachedStartSec = NaN;
let cachedEndSec = NaN;
let cachedPtsPerSec = 0;

let waveImgBitmap: ImageBitmap | null = null;
let bitmapForPeaks: Float32Array | null = null;
let bitmapCanvasH = 0;
let bitmapPixelW = 0;
let bitmapBuildInFlight = false;


function requiredPtsPerSec(): number {
  const canvas = canvasEl.value;
  const zoomSec = viewEndSec - viewStartSec;
  if (!canvas || zoomSec <= 0) return 0;
  // Use physical pixels (canvas.width = clientWidth * dpr) so that on Retina
  // displays, zoom levels that need finer data than the dense LOD provides
  // correctly fall through to on-demand fetches instead of serving blurry data.
  return canvas.width / zoomSec;
}

function cacheCoversView(): boolean {
  if (!cachedPeaks) return false;
  const required = requiredPtsPerSec();
  return (
    cachedStartSec <= viewStartSec + 1e-6 &&
    cachedEndSec >= viewEndSec - 1e-6 &&
    cachedPtsPerSec >= required * 0.9
  );
}

function ensureCachedFromDense(): boolean {
  if (cacheCoversView() && cachedPtsPerSec === props.denseSpectralRate) return true;
  const dense = props.denseSpectralData;
  const denseRate = props.denseSpectralRate;
  if (!dense || denseRate <= 0) return false;
  if (denseRate < requiredPtsPerSec() * 0.9) return false;

  const viewSpan = viewEndSec - viewStartSec;
  const pad = viewSpan * OVERSCAN_FACTOR;
  const rStart = Math.max(0, viewStartSec - pad);
  const rEnd = Math.min(trackDuration, viewEndSec + pad);
  const totalPoints = (dense.length / 4) | 0;
  const startIdx = Math.max(0, Math.floor(rStart * denseRate));
  const endIdx = Math.min(totalPoints, Math.ceil(rEnd * denseRate));
  if (endIdx <= startIdx) return false;

  cachedPeaks = dense.subarray(startIdx * 4, endIdx * 4);
  cachedStartSec = startIdx / denseRate;
  cachedEndSec = endIdx / denseRate;
  cachedPtsPerSec = denseRate;
  return true;
}

let isFetching = false;
let pendingFetch = false;
let fetchDebounceTimer = 0;

async function doFetchOnDemand() {
  if (!props.trackData) return;
  isFetching = true;
  const viewSpan = viewEndSec - viewStartSec;
  const pad = viewSpan * OVERSCAN_FACTOR;
  const rStart = Math.max(0, viewStartSec - pad);
  const rEnd = Math.min(trackDuration, viewEndSec + pad);
  const rate = requiredPtsPerSec();
  if (rate <= 0 || rEnd <= rStart) { isFetching = false; return; }
  const numPoints = Math.max(64, Math.ceil((rEnd - rStart) * rate));
  try {
    const result = await props.getSpectralWaveformRegion(rStart, rEnd, numPoints);
    // Only apply if this fetch still covers the current view — a later
    // pan/zoom might have moved us outside the fetched range.
    if (rStart <= viewStartSec + 1e-6 && rEnd >= viewEndSec - 1e-6) {
      cachedPeaks = new Float32Array(result);
      cachedStartSec = rStart;
      cachedEndSec = rEnd;
      cachedPtsPerSec = rate;
    }
  } catch (err) {
    console.error('[WaveformDisplay] spectral fetch failed:', err);
  }
  isFetching = false;
  if (pendingFetch) {
    pendingFetch = false;
    // Re-check: dense LOD may have arrived, or the view may have moved
    // back inside the current cache — in either case we skip the IPC.
    if (!cacheCoversView() && !ensureCachedFromDense()) {
      doFetchOnDemand();
    }
  }
}

function ensurePeaks() {
  if (ensureCachedFromDense()) return;
  if (cacheCoversView()) return;
  if (props.denseSpectralRate <= 0) return;
  if (isFetching) { pendingFetch = true; return; }
  clearTimeout(fetchDebounceTimer);
  fetchDebounceTimer = window.setTimeout(() => {
    if (cacheCoversView() || ensureCachedFromDense()) return;
    doFetchOnDemand();
  }, 80);
}

function ensureBitmap(canvasW: number, canvasH: number) {
  if (!cachedPeaks) return;
  const cacheSpan = cachedEndSec - cachedStartSec;
  const viewSpan = viewEndSec - viewStartSec;
  if (cacheSpan <= 0 || viewSpan <= 0 || canvasW <= 0 || canvasH <= 0) return;

  const screenPxPerSec = canvasW / viewSpan;
  const dataPts = (cachedPeaks.length / 4) | 0;
  let bitmapW = Math.ceil(cacheSpan * screenPxPerSec);
  bitmapW = Math.min(bitmapW, dataPts, MAX_BITMAP_PX);
  if (bitmapW < 1) return;

  const sameSource = bitmapForPeaks === cachedPeaks;
  const sameSize = bitmapCanvasH === canvasH;
  const closeWidth = sameSize && Math.abs(bitmapPixelW - bitmapW) <= bitmapW * 0.15;
  if (sameSource && sameSize && closeWidth) return;
  if (bitmapBuildInFlight) return;

  bitmapForPeaks = cachedPeaks;
  bitmapCanvasH = canvasH;
  bitmapPixelW = bitmapW;
  bitmapBuildInFlight = true;
  const imgData = buildWaveformImageData(bitmapW, canvasH, cachedPeaks, WAVEFORM_AMP_SCALE);
  createImageBitmap(imgData)
    .then(bmp => { waveImgBitmap = bmp; })
    .catch(() => {})
    .finally(() => { bitmapBuildInFlight = false; });
}

function pxToSec(px: number): number {
  const canvas = canvasEl.value;
  if (!canvas) return 0;
  return viewStartSec + (px / canvas.clientWidth) * (viewEndSec - viewStartSec);
}

function secToPx(sec: number): number {
  const canvas = canvasEl.value;
  if (!canvas) return 0;
  const span = viewEndSec - viewStartSec;
  return ((sec - viewStartSec) / span) * canvas.clientWidth;
}

function drawWaveform() {
  const canvas = canvasEl.value;
  if (!canvas || !props.trackData) return;

  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (w === 0 || h === 0) return;

  if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
    canvas.width = w * dpr;
    canvas.height = h * dpr;
    ctx.scale(dpr, dpr);
  }

  ensureBitmap(canvas.width, canvas.height);

  ctx.fillStyle = '#0a0a0a';
  ctx.fillRect(0, 0, w, h);
  if (waveImgBitmap && !isNaN(cachedStartSec)) {
    const bitmapLeftPx = secToPx(cachedStartSec);
    const bitmapRightPx = secToPx(cachedEndSec);
    const bitmapWidthPx = bitmapRightPx - bitmapLeftPx;
    if (bitmapWidthPx > 0) {
      ctx.drawImage(waveImgBitmap, bitmapLeftPx, 0, bitmapWidthPx, h);
    }
  }

  drawRuler(ctx, w, h);
  drawDownbeatMarker(ctx, w, h);
  drawCueMarker(ctx, w, h);
  drawPlayhead(ctx, w, h);
}

const MIN_LINE_SPACING_PX = 6;

function drawRuler(ctx: CanvasRenderingContext2D, w: number, h: number) {
  if (!props.trackBpm || props.trackBpm <= 0) return;
  const beatDurSec = 60 / props.trackBpm;
  const viewSpan = viewEndSec - viewStartSec;
  const pxPerBeat = (beatDurSec / viewSpan) * w;

  // Find the coarsest subdivision that puts lines at least MIN_LINE_SPACING_PX apart.
  // Steps: 1 beat → 4 beats (bar) → 16 beats (phrase) → 64 → ...
  let step = 1;
  while (pxPerBeat * step < MIN_LINE_SPACING_PX) {
    step *= 4;
  }
  if (pxPerBeat * step < 1) return;

  const beatOffset = props.beatOffset;
  const firstBeat = Math.ceil((viewStartSec - beatOffset) / beatDurSec);
  const lastBeat = Math.floor((viewEndSec - beatOffset) / beatDurSec);

  for (let b = firstBeat; b <= lastBeat; b++) {
    if (b % step !== 0) continue;
    const t = beatOffset + b * beatDurSec;
    const x = secToPx(t);
    if (x < 0 || x > w) continue;

    const isPhrase = b % 16 === 0;
    const isBar = b % 4 === 0;

    if (isPhrase) {
      ctx.strokeStyle = props.accent;
      ctx.globalAlpha = 0.5;
      ctx.lineWidth = 1;
    } else if (isBar) {
      ctx.strokeStyle = props.accent;
      ctx.globalAlpha = 0.25;
      ctx.lineWidth = 1;
    } else {
      ctx.strokeStyle = '#ffffff';
      ctx.globalAlpha = 0.12;
      ctx.lineWidth = 0.5;
    }

    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, h);
    ctx.stroke();
    ctx.globalAlpha = 1;
  }
}

let rafId = 0;
let lastZoomTime = 0;
const ZOOM_COOLDOWN_MS = 150;

function drawPlayhead(ctx: CanvasRenderingContext2D, w: number, h: number) {
  const sec = props.getPlayheadPosition();
  const x = secToPx(sec);
  if (x < 0 || x > w) return;
  const isPlaying = props.getTrackPosition() !== null;
  const color = isPlaying ? '#ffffff' : '#ef4444';
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = PLAYHEAD_LINE_WIDTH;
  ctx.globalAlpha = PLAYHEAD_ALPHA;
  ctx.beginPath();
  ctx.moveTo(x, 0);
  ctx.lineTo(x, h);
  ctx.stroke();
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.moveTo(x - PLAYHEAD_ARROW_HALF, 0);
  ctx.lineTo(x + PLAYHEAD_ARROW_HALF, 0);
  ctx.lineTo(x, PLAYHEAD_ARROW_HEIGHT);
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

const MARKER_TRI_W = 7;
const MARKER_TRI_H = 11;
const MARKER_LINE_WIDTH = 1.5;

function drawDownbeatMarker(ctx: CanvasRenderingContext2D, w: number, h: number) {
  const x = secToPx(props.beatOffset);
  if (x < -MARKER_TRI_W || x > w + MARKER_TRI_W) return;
  ctx.save();
  ctx.strokeStyle = '#ffffff';
  ctx.fillStyle = '#ffffff';
  ctx.globalAlpha = 0.85;
  ctx.lineWidth = MARKER_LINE_WIDTH;
  ctx.beginPath();
  ctx.moveTo(x, 0);
  ctx.lineTo(x, h);
  ctx.stroke();
  ctx.beginPath();
  ctx.moveTo(x - MARKER_TRI_W, 0);
  ctx.lineTo(x + MARKER_TRI_W, 0);
  ctx.lineTo(x, MARKER_TRI_H);
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

function drawCueMarker(ctx: CanvasRenderingContext2D, w: number, h: number) {
  const x = secToPx(props.cuePoint);
  if (x < -MARKER_TRI_W || x > w + MARKER_TRI_W) return;
  ctx.save();
  ctx.fillStyle = '#eab308';
  ctx.globalAlpha = 0.9;
  ctx.beginPath();
  ctx.moveTo(x - MARKER_TRI_W, h);
  ctx.lineTo(x + MARKER_TRI_W, h);
  ctx.lineTo(x, h - MARKER_TRI_H);
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

function rafLoop() {
  applyPendingDrag();
  // Sync path only: keeps the dense-LOD slice up to date as the user drags
  // past the current cache range. On-demand fetches (IPC) are reserved for
  // explicit events so we don't flood the backend during a fast drag.
  ensureCachedFromDense();
  drawWaveform();
  rafId = requestAnimationFrame(rafLoop);
}

// mousemove is batched to rAF: only the latest clientX is stored per frame.
// Avoids redundant work when mousemove fires faster than display refresh (trackpads at 120 Hz).
let dragging: 'pan' | null = null;
let panStartX = 0;
let panStartViewSec = 0;
let mouseDownPx = 0;
let dragRectLeft = 0;
let dragRectWidth = 0;
let pendingDragX: number | null = null;
const CLICK_THRESHOLD_PX = 5;

function onMouseDown(e: MouseEvent) {
  const canvas = canvasEl.value;
  if (!canvas) return;
  const rect = canvas.getBoundingClientRect();
  dragRectLeft = rect.left;
  dragRectWidth = rect.width;
  const px = e.clientX - dragRectLeft;
  mouseDownPx = px;
  dragging = 'pan';
  panStartX = px;
  panStartViewSec = viewStartSec;

  window.addEventListener('mousemove', onMouseMoveWindow);
  window.addEventListener('mouseup', onMouseUp);
}

function applyPendingDrag() {
  if (pendingDragX === null || dragging !== 'pan' || dragRectWidth === 0) return;
  const px = pendingDragX - dragRectLeft;
  const viewSpan = viewEndSec - viewStartSec;
  const deltaSec = -((px - panStartX) / dragRectWidth) * viewSpan;
  const [s, e] = clampView(panStartViewSec + deltaSec, viewSpan);
  viewStartSec = s;
  viewEndSec = e;
  pendingDragX = null;
}

function onMouseMoveWindow(e: MouseEvent) {
  if (!dragging) return;
  pendingDragX = e.clientX;
}

function onWheel(e: WheelEvent) {
  const canvas = canvasEl.value;
  if (!canvas) return;

  const rect = canvas.getBoundingClientRect();
  const frac = (e.clientX - rect.left) / rect.width;
  const anchorSec = viewStartSec + frac * (viewEndSec - viewStartSec);

  if (Math.abs(e.deltaX) > 2 && Math.abs(e.deltaX) >= Math.abs(e.deltaY)) {
    const viewSpan = viewEndSec - viewStartSec;
    const deltaSec = (e.deltaX / rect.width) * viewSpan;
    const [s, end] = clampView(viewStartSec + deltaSec, viewSpan);
    viewStartSec = s;
    viewEndSec = end;
    ensurePeaks();
  } else if (e.deltaY !== 0) {
    const now = Date.now();
    if (now - lastZoomTime > ZOOM_COOLDOWN_MS) {
      if (e.deltaY < 0) zoomIn(anchorSec);
      else zoomOut(anchorSec);
      lastZoomTime = now;
    }
  }
}

function onMouseUp(e: MouseEvent) {
  pendingDragX = null;
  const px = e.clientX - dragRectLeft;
  const wasDragging = dragging === 'pan';
  const moved = Math.abs(px - mouseDownPx) >= CLICK_THRESHOLD_PX;
  dragging = null;
  window.removeEventListener('mousemove', onMouseMoveWindow);
  window.removeEventListener('mouseup', onMouseUp);
  if (wasDragging && !moved) {
    emit('seek', Math.max(0, Math.min(pxToSec(mouseDownPx), trackDuration)));
  } else if (wasDragging) {
    ensurePeaks();
  }
}

let resizeObserver: ResizeObserver | null = null;

watch(canvasEl, (el) => {
  if (el && !resizeObserver) {
    resizeObserver = new ResizeObserver(() => {
      ensurePeaks();
    });
    resizeObserver.observe(el.parentElement!);
  }
});

onMounted(() => {
  rafId = requestAnimationFrame(rafLoop);
});

onUnmounted(() => {
  resizeObserver?.disconnect();
  cancelAnimationFrame(rafId);
  clearTimeout(fetchDebounceTimer);
});

watch(
  () => props.trackData,
  (data) => {
    if (data) {
      trackDuration = data.duration;
      zoomIdx.value = ZOOM_LEVELS_SEC.indexOf(DEFAULT_ZOOM_SEC);
      const dur = viewDurationSec();
      viewStartSec = 0;
      viewEndSec = Math.min(dur, trackDuration);
      cachedPeaks = null;
      cachedStartSec = NaN;
      cachedEndSec = NaN;
      cachedPtsPerSec = 0;
      bitmapForPeaks = null;
      waveImgBitmap = null;
      ensurePeaks();
    } else {
      trackDuration = 0;
      cachedPeaks = null;
      cachedStartSec = NaN;
      cachedEndSec = NaN;
      cachedPtsPerSec = 0;
      bitmapForPeaks = null;
      waveImgBitmap = null;
    }
  },
  { immediate: true }
);

watch(
  () => props.denseSpectralData,
  (data) => {
    if (data) ensurePeaks();
  }
);
</script>

<style scoped>
.waveform {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  background: #0a0a0a;
  position: relative;
}

.waveform--drag-over {
  outline: 2px dashed v-bind(accent);
  outline-offset: -4px;
}

.waveform__empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: #444;
}

.waveform__empty-text {
  color: var(--color-muted);
  font-size: 0.7rem;
  letter-spacing: 0.12em;
  opacity: 0.6;
}

.waveform__canvas {
  flex: 1;
  width: 100%;
  display: block;
  cursor: crosshair;
}

.waveform__controls {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  border-top: 1px solid #1e1e1e;
  background: #0d0d0d;
}

.waveform__bpm-readout {
  font-size: 0.85rem;
  font-weight: 700;
  color: v-bind(accent);
  margin-left: auto;
  letter-spacing: 0.05em;
}

.waveform__zoom {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-left: auto;
}

.waveform__bpm-readout + .waveform__zoom {
  margin-left: 0;
}

.waveform__zoom-btn {
  background: #1a1a1a;
  border: 1px solid #2a2a2a;
  color: #aaa;
  font-family: var(--font-mono);
  font-size: 1rem;
  width: 26px;
  height: 26px;
  border-radius: 3px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  line-height: 1;
}

.waveform__zoom-btn:hover {
  border-color: #555;
  color: #eee;
}

.waveform__zoom-label {
  font-size: 0.65rem;
  letter-spacing: 0.1em;
  color: #555;
  min-width: 24px;
  text-align: center;
}

</style>
