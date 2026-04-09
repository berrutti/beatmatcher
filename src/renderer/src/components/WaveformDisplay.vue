<template>
  <div
    class="waveform"
    :class="{ 'waveform--drag-over': props.isDragOver }"
  >
    <!-- Empty state -->
    <div v-if="!props.trackData" class="waveform__empty">
      <button class="waveform__load-btn" @click="emit('openFileDialog')">LOAD TRACK</button>
    </div>

    <!-- Waveform canvas -->
    <template v-if="props.trackData">
      <canvas
        ref="canvasEl"
        class="waveform__canvas"
        @mousedown="onMouseDown"
        @mousemove="onMouseMoveCanvas"
        @wheel.prevent="onWheel"
        @contextmenu.prevent
      />

      <!-- Controls bar -->
      <div class="waveform__controls">
        <span class="waveform__bpm-readout" v-if="props.trackBpm > 0">
          {{ props.trackBpm.toFixed(1) }} BPM
        </span>

        <!-- Zoom controls -->
        <div class="waveform__zoom">
          <button class="waveform__zoom-btn" @click="() => zoomOut()">−</button>
          <span class="waveform__zoom-label">{{ zoomLabel }}</span>
          <button class="waveform__zoom-btn" @click="() => zoomIn()">+</button>
        </div>

        <button class="waveform__set-bpm-btn" @click="emit('openFileDialog')">LOAD</button>
        <button class="waveform__set-bpm-btn" @click="emit('requestBpmInput')">SET BPM</button>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue';
import type { TrackData } from '@renderer/stores/decks';

const props = defineProps<{
  accent: string;
  trackData: TrackData | null;
  isDragOver: boolean;
  trackBpm: number | null;
  beatOffset: number;
  cuePoint: number;
  getTrackPosition: () => number | null;
  getWaveformRegion: (startSec: number, endSec: number, numPoints: number) => Promise<number[]>;
}>();

const emit = defineEmits<{
  openFileDialog: [];
  setBeatOffset: [sec: number];
  seek: [sec: number];
  requestBpmInput: [];
}>();

const accent = computed(() => props.accent);
const HANDLE_W = 8;
const WAVEFORM_AMP_SCALE = 0.9;
const PLAYHEAD_LINE_WIDTH = 1.5;
const PLAYHEAD_ALPHA = 0.9;
const PLAYHEAD_ARROW_HALF = 5;
const PLAYHEAD_ARROW_HEIGHT = 8;

// Zoom levels in seconds of visible track
const ZOOM_LEVELS_SEC = [0.25, 0.5, 1, 2, 5, 10, 20, 30, 60, 120, 300];
const DEFAULT_ZOOM_SEC = 10;

const canvasEl = ref<HTMLCanvasElement | null>(null);

let trackDuration = 0;

// Visible window in seconds
const viewStartSec = ref(0);
const viewEndSec = ref(0);

// Current zoom index
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
  const center = anchorSec ?? (viewStartSec.value + viewEndSec.value) / 2;
  const [s, e] = clampView(center - dur / 2, dur);
  viewStartSec.value = s;
  viewEndSec.value = e;
  drawWaveform();
}

// Lower index = fewer seconds = more detail = zoom in
function zoomIn(anchorSec?: number) {
  setZoomCentered(zoomIdx.value - 1, anchorSec);
}
function zoomOut(anchorSec?: number) {
  setZoomCentered(zoomIdx.value + 1, anchorSec);
}

// ── Peak fetch ────────────────────────────────────────────────
// Peaks are fetched on-demand from the Rust backend for the exact visible
// region at the canvas pixel width. This gives full resolution at any zoom
// level without sending a pre-baked array of fixed chunk size.

let visiblePeaks: Float32Array | null = null;
let peaksCachedViewStart = NaN;
let peaksCachedViewEnd = NaN;
let peaksCachedWidth = 0;
let isFetching = false;
let pendingFetch = false;

async function fetchVisiblePeaks() {
  if (isFetching) {
    pendingFetch = true;
    return;
  }
  const canvas = canvasEl.value;
  if (!canvas || !props.trackData) return;
  const w = canvas.clientWidth;
  if (w === 0) return;

  const viewStart = viewStartSec.value;
  const viewEnd = viewEndSec.value;

  if (
    peaksCachedWidth === w &&
    peaksCachedViewStart === viewStart &&
    peaksCachedViewEnd === viewEnd
  )
    return;

  isFetching = true;
  pendingFetch = false;
  try {
    const result = await props.getWaveformRegion(viewStart, viewEnd, w);
    // Discard if the view changed while we were waiting
    if (viewStart === viewStartSec.value && viewEnd === viewEndSec.value) {
      visiblePeaks = new Float32Array(result);
      peaksCachedViewStart = viewStart;
      peaksCachedViewEnd = viewEnd;
      peaksCachedWidth = w;
    }
  } catch {}
  isFetching = false;
  if (pendingFetch) {
    fetchVisiblePeaks();
  }
}

// ── Coordinate helpers ────────────────────────────────────────

function pxToSec(px: number): number {
  const canvas = canvasEl.value;
  if (!canvas) return 0;
  return viewStartSec.value + (px / canvas.clientWidth) * (viewEndSec.value - viewStartSec.value);
}

function secToPx(sec: number): number {
  const canvas = canvasEl.value;
  if (!canvas) return 0;
  const span = viewEndSec.value - viewStartSec.value;
  return ((sec - viewStartSec.value) / span) * canvas.clientWidth;
}

// ── Canvas drawing ────────────────────────────────────────────

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

  fetchVisiblePeaks(); // fire-and-forget; draws with current peaks, updates on next frame after fetch
  if (!visiblePeaks) return;

  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = '#0a0a0a';
  ctx.fillRect(0, 0, w, h);

  const mid = h / 2;

  // Waveform (dim, outside loop)
  for (let x = 0; x < w; x++) {
    const amp = visiblePeaks[x] * mid * WAVEFORM_AMP_SCALE;
    ctx.fillStyle = '#333';
    ctx.fillRect(x, mid - amp, 1, amp * 2);
  }

  // Beat grid
  drawRuler(ctx, w, h);
  // Downbeat marker (draggable grid start) and cue point
  drawDownbeatMarker(ctx, w, h);
  drawCueMarker(ctx, w, h);
  // Playhead
  drawPlayhead(ctx, w, h);
}

// Index in ZOOM_LEVELS_SEC at which individual beats are hidden (30s and above)
const BEAT_HIDE_ZOOM_IDX = ZOOM_LEVELS_SEC.indexOf(30);

function drawRuler(ctx: CanvasRenderingContext2D, w: number, h: number) {
  if (!props.trackBpm || props.trackBpm <= 0) return;
  const beatDurSec = 60 / props.trackBpm;
  const beatOffset = props.beatOffset;
  const zoomedOut = zoomIdx.value >= BEAT_HIDE_ZOOM_IDX;

  const firstBeat = Math.ceil((viewStartSec.value - beatOffset) / beatDurSec);
  const lastBeat = Math.floor((viewEndSec.value - beatOffset) / beatDurSec);

  for (let b = firstBeat; b <= lastBeat; b++) {
    const isPhrase = b % 16 === 0;
    const isBar = b % 4 === 0;

    // At 30s+ zoom, skip individual beats
    if (zoomedOut && !isBar) continue;

    const t = beatOffset + b * beatDurSec;
    const x = secToPx(t);
    if (x < 0 || x > w) continue;

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

// ── Playhead ──────────────────────────────────────────────────

let rafId = 0;
let lastZoomTime = 0;
const ZOOM_COOLDOWN_MS = 150;

function drawPlayhead(ctx: CanvasRenderingContext2D, w: number, h: number) {
  const sec = props.getTrackPosition();
  if (sec === null) return;
  const x = secToPx(sec);
  if (x < 0 || x > w) return;
  ctx.save();
  ctx.strokeStyle = '#ffffff';
  ctx.lineWidth = PLAYHEAD_LINE_WIDTH;
  ctx.globalAlpha = PLAYHEAD_ALPHA;
  ctx.beginPath();
  ctx.moveTo(x, 0);
  ctx.lineTo(x, h);
  ctx.stroke();
  ctx.fillStyle = '#ffffff';
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
  ctx.strokeStyle = '#eab308';
  ctx.fillStyle = '#eab308';
  ctx.globalAlpha = 0.9;
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

function rafLoop() {
  drawWaveform();
  rafId = requestAnimationFrame(rafLoop);
}

// ── Drag interaction ──────────────────────────────────────────

type DragHandle = 'pan' | 'beatOffset' | null;
const dragging = ref<DragHandle>(null);
const panStartX = ref(0);
const panStartViewSec = ref(0);
let mouseDownPx = 0;
const CLICK_THRESHOLD_PX = 5;

function hitTest(px: number): DragHandle {
  const bx = secToPx(props.beatOffset);
  if (Math.abs(px - bx) < HANDLE_W + 4) return 'beatOffset';
  return null;
}

function canvasPx(clientX: number): number {
  const canvas = canvasEl.value;
  if (!canvas) return 0;
  return clientX - canvas.getBoundingClientRect().left;
}

function onMouseDown(e: MouseEvent) {
  const px = canvasPx(e.clientX);
  mouseDownPx = px;
  const hit = hitTest(px);

  if (hit === 'beatOffset') {
    dragging.value = 'beatOffset';
  } else {
    dragging.value = 'pan';
    panStartX.value = px;
    panStartViewSec.value = viewStartSec.value;
  }

  window.addEventListener('mousemove', onMouseMoveWindow);
  window.addEventListener('mouseup', onMouseUp);
}

function applyDrag(clientX: number) {
  const px = canvasPx(clientX);

  if (dragging.value === 'pan') {
    const viewSpan = viewEndSec.value - viewStartSec.value;
    const w = canvasEl.value!.clientWidth;
    const deltaSec = -((px - panStartX.value) / w) * viewSpan;
    const [s, e] = clampView(panStartViewSec.value + deltaSec, viewSpan);
    viewStartSec.value = s;
    viewEndSec.value = e;
    drawWaveform();
    return;
  }

  if (dragging.value === 'beatOffset') {
    const sec = Math.max(0, Math.min(pxToSec(px), trackDuration));
    emit('setBeatOffset', sec);
    drawWaveform();
    return;
  }
}

// Canvas hover — only updates cursor when not dragging
function onMouseMoveCanvas(e: MouseEvent) {
  if (dragging.value) return;
  const px = canvasPx(e.clientX);
  const hit = hitTest(px);
  const canvas = canvasEl.value;
  if (!canvas) return;
  canvas.style.cursor = hit === 'beatOffset' ? 'ew-resize' : 'crosshair';
}

// Window-level move — fires even when mouse leaves canvas
function onMouseMoveWindow(e: MouseEvent) {
  if (!dragging.value) return;
  applyDrag(e.clientX);
}

function onWheel(e: WheelEvent) {
  const canvas = canvasEl.value;
  if (!canvas) return;

  const rect = canvas.getBoundingClientRect();
  const frac = (e.clientX - rect.left) / rect.width;
  const anchorSec = viewStartSec.value + frac * (viewEndSec.value - viewStartSec.value);

  if (Math.abs(e.deltaX) > 2 && Math.abs(e.deltaX) >= Math.abs(e.deltaY)) {
    // Horizontal swipe: pan
    const viewSpan = viewEndSec.value - viewStartSec.value;
    const deltaSec = (e.deltaX / canvas.clientWidth) * viewSpan;
    const [s, end] = clampView(viewStartSec.value + deltaSec, viewSpan);
    viewStartSec.value = s;
    viewEndSec.value = end;
    drawWaveform();
  } else if (e.deltaY !== 0) {
    // Zoom (mouse wheel or pinch): cooldown prevents jumping multiple levels per gesture
    const now = Date.now();
    if (now - lastZoomTime > ZOOM_COOLDOWN_MS) {
      if (e.deltaY < 0) zoomIn(anchorSec);
      else zoomOut(anchorSec);
      lastZoomTime = now;
    }
  }
}

function onMouseUp(e: MouseEvent) {
  const px = canvasPx(e.clientX);
  if (dragging.value === 'pan' && Math.abs(px - mouseDownPx) < CLICK_THRESHOLD_PX) {
    emit('seek', Math.max(0, Math.min(pxToSec(mouseDownPx), trackDuration)));
  }
  dragging.value = null;
  window.removeEventListener('mousemove', onMouseMoveWindow);
  window.removeEventListener('mouseup', onMouseUp);
}

// ── Resize handling ───────────────────────────────────────────

let resizeObserver: ResizeObserver | null = null;

watch(canvasEl, (el) => {
  if (el && !resizeObserver) {
    resizeObserver = new ResizeObserver(() => {
      drawWaveform();
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
});

watch(
  () => props.trackData,
  (data) => {
    if (data) {
      trackDuration = data.duration;
      peaksCachedViewStart = NaN; // invalidate cache so next fetch always runs
      zoomIdx.value = ZOOM_LEVELS_SEC.indexOf(DEFAULT_ZOOM_SEC);
      const dur = viewDurationSec();
      viewStartSec.value = 0;
      viewEndSec.value = Math.min(dur, trackDuration);
      fetchVisiblePeaks();
    } else {
      trackDuration = 0;
      visiblePeaks = null;
      peaksCachedViewStart = NaN;
    }
  }
);

// Re-fetch whenever the visible window changes (zoom or pan)
watch([viewStartSec, viewEndSec], () => {
  peaksCachedViewStart = NaN;
  fetchVisiblePeaks();
});
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

.waveform__load-btn {
  background: #1a1a1a;
  border: 1px solid #333;
  color: #aaa;
  font-family: var(--font-mono);
  font-size: 0.7rem;
  letter-spacing: 0.15em;
  padding: 8px 20px;
  border-radius: 4px;
  cursor: pointer;
}

.waveform__load-btn:hover {
  border-color: #555;
  color: #eee;
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

.waveform__set-bpm-btn {
  background: transparent;
  border: 1px solid #333;
  color: #777;
  font-family: var(--font-mono);
  font-size: 0.6rem;
  letter-spacing: 0.15em;
  padding: 4px 10px;
  border-radius: 3px;
  cursor: pointer;
}

.waveform__set-bpm-btn:hover {
  border-color: #555;
  color: #aaa;
}
</style>
