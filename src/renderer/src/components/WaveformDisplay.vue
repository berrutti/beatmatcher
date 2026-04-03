<template>
  <div
    class="waveform"
    :class="{ 'waveform--drag-over': isDragOver }"
    @dragover.prevent="isDragOver = true"
    @dragleave="isDragOver = false"
    @drop.prevent="onDrop"
  >
    <!-- Empty state -->
    <div v-if="!props.buffer" class="waveform__empty">
      <div class="waveform__drop-icon">⊕</div>
      <p class="waveform__drop-hint">Drop audio file here</p>
      <p class="waveform__drop-hint waveform__drop-hint--sub">or</p>
      <button class="waveform__load-btn" @click="openFileDialog">LOAD TRACK</button>
    </div>

    <!-- Waveform canvas -->
    <template v-if="props.buffer">
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
        <span class="waveform__ctrl-label">LOOP</span>
        <button
          class="waveform__beat-btn"
          :class="{ 'waveform__beat-btn--active': props.loopBeats === 16 }"
          @click="emit('setBeats', 16)"
        >
          16
        </button>
        <button
          class="waveform__beat-btn"
          :class="{ 'waveform__beat-btn--active': props.loopBeats === 32 }"
          @click="emit('setBeats', 32)"
        >
          32
        </button>

        <button
          class="waveform__lock-btn"
          :class="{ 'waveform__lock-btn--unlocked': !regionLocked }"
          @click="regionLocked = !regionLocked"
          :title="
            regionLocked
              ? 'Region locked. Click to unlock and resize.'
              : 'Region unlocked. Click to lock.'
          "
        >
          {{ regionLocked ? '🔒' : '🔓' }}
        </button>

        <span class="waveform__bpm-readout" v-if="props.loopRegion">
          {{ props.inferredBpm.toFixed(1) }} BPM
        </span>

        <!-- Zoom controls -->
        <div class="waveform__zoom">
          <button class="waveform__zoom-btn" @click="() => zoomOut()">−</button>
          <span class="waveform__zoom-label">{{ zoomLabel }}</span>
          <button class="waveform__zoom-btn" @click="() => zoomIn()">+</button>
        </div>

        <button class="waveform__set-bpm-btn" @click="openFileDialog">LOAD</button>
        <button class="waveform__set-bpm-btn" @click="emit('requestBpmInput')">SET BPM</button>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue';
import type { LoopRegion } from '@renderer/audio/LoopEngine';

const props = defineProps<{
  accent: string;
  buffer: AudioBuffer | null;
  loopRegion: LoopRegion | null;
  loopBeats: 16 | 32;
  inferredBpm: number;
  getTrackPosition: () => number | null;
}>();

const emit = defineEmits<{
  load: [file: File];
  setRegion: [region: LoopRegion];
  moveRegion: [startSec: number];
  setBeats: [beats: 16 | 32];
  requestBpmInput: [];
}>();

const accent = computed(() => props.accent);
const regionLocked = ref(true);
const HANDLE_W = 8;
const HANDLE_CIRCLE_RADIUS = 6;
const WAVEFORM_AMP_SCALE = 0.9;
const REGION_IN_LOOP_ALPHA = 0.8;
const BPM_LABEL_FONT_SIZE = 11;
const MIN_NEW_REGION_SEC = 0.001;
const MIN_REGION_SEC = 0.01;
const PLAYHEAD_LINE_WIDTH = 1.5;
const PLAYHEAD_ALPHA = 0.9;
const PLAYHEAD_ARROW_HALF = 5;
const PLAYHEAD_ARROW_HEIGHT = 8;

// Zoom levels in seconds of visible track
const ZOOM_LEVELS_SEC = [0.25, 0.5, 1, 2, 5, 10, 20, 30, 60, 120, 300];
const DEFAULT_ZOOM_SEC = 10;

const canvasEl = ref<HTMLCanvasElement | null>(null);
const isDragOver = ref(false);

// Raw PCM channel data (kept in memory for high-res zoom)
let rawChannel: Float32Array | null = null;
let sampleRate = 0;
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

// ── File loading ──────────────────────────────────────────────

function onDrop(e: DragEvent) {
  isDragOver.value = false;
  const file = e.dataTransfer?.files[0];
  if (file) emit('load', file);
}

function openFileDialog() {
  const input = document.createElement('input');
  input.type = 'file';
  input.accept = 'audio/*';
  input.onchange = () => {
    if (input.files?.[0]) emit('load', input.files[0]);
  };
  input.click();
}

// ── Peak extraction ───────────────────────────────────────────

let visiblePeaks: Float32Array | null = null;
let peaksCachedViewStart = NaN;
let peaksCachedViewEnd = NaN;
let peaksCachedWidth = 0;

function buildFullPeaks() {
  if (!props.buffer) return;
  rawChannel = props.buffer.getChannelData(0);
  sampleRate = props.buffer.sampleRate;
  peaksCachedViewStart = NaN; // invalidate cache
}

function ensureVisiblePeaks(w: number) {
  if (!rawChannel) return;
  const viewStart = viewStartSec.value;
  const viewEnd = viewEndSec.value;
  if (
    visiblePeaks !== null &&
    peaksCachedWidth === w &&
    peaksCachedViewStart === viewStart &&
    peaksCachedViewEnd === viewEnd
  )
    return;

  visiblePeaks = new Float32Array(w);
  const totalSamples = rawChannel.length;
  for (let x = 0; x < w; x++) {
    const tStart = viewStart + (x / w) * (viewEnd - viewStart);
    const tEnd = viewStart + ((x + 1) / w) * (viewEnd - viewStart);
    const iStart = Math.max(0, Math.floor(tStart * sampleRate));
    const iEnd = Math.min(totalSamples, Math.ceil(tEnd * sampleRate));
    let max = 0;
    for (let i = iStart; i < iEnd; i++) {
      const abs = Math.abs(rawChannel[i]);
      if (abs > max) max = abs;
    }
    visiblePeaks[x] = max;
  }
  peaksCachedViewStart = viewStart;
  peaksCachedViewEnd = viewEnd;
  peaksCachedWidth = w;
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
  if (!canvas || !rawChannel) return;

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

  ensureVisiblePeaks(w);
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

  // Loop region overlay
  const region = props.loopRegion;
  if (region) {
    const sx = secToPx(region.startSec);
    const ex = secToPx(region.endSec);

    // Clamp to canvas
    const visStart = Math.max(0, sx);
    const visEnd = Math.min(w, ex);

    if (visEnd > visStart) {
      // Tinted overlay
      ctx.fillStyle = `${props.accent}22`;
      ctx.fillRect(visStart, 0, visEnd - visStart, h);

      // Bright waveform inside loop
      ctx.globalAlpha = REGION_IN_LOOP_ALPHA;
      for (let x = Math.floor(visStart); x < Math.ceil(visEnd); x++) {
        const amp = visiblePeaks[x] * mid * WAVEFORM_AMP_SCALE;
        ctx.fillStyle = props.accent;
        ctx.fillRect(x, mid - amp, 1, amp * 2);
      }
      ctx.globalAlpha = 1;
    }

    // Handles (only if visible)
    if (sx >= -HANDLE_W && sx <= w + HANDLE_W) drawHandle(ctx, sx, h);
    if (ex >= -HANDLE_W && ex <= w + HANDLE_W) drawHandle(ctx, ex, h);

    // BPM label
    const labelX = Math.max(visStart + 10, 10);
    ctx.fillStyle = '#ffffff99';
    ctx.font = `bold ${BPM_LABEL_FONT_SIZE}px monospace`;
    ctx.fillText(`${props.loopBeats} beats · ${props.inferredBpm.toFixed(1)} BPM`, labelX, 18);
  }

  // Time ruler
  drawRuler(ctx, w, h);

  // Playhead
  drawPlayhead(ctx, w, h);
}

function drawHandle(ctx: CanvasRenderingContext2D, x: number, h: number) {
  ctx.fillStyle = props.accent;
  ctx.fillRect(x - HANDLE_W / 2, 0, HANDLE_W, h);
  ctx.fillStyle = '#fff';
  const mid = h / 2;
  ctx.beginPath();
  ctx.arc(x, mid, HANDLE_CIRCLE_RADIUS, 0, Math.PI * 2);
  ctx.fill();
}

function drawRuler(ctx: CanvasRenderingContext2D, w: number, h: number) {
  const region = props.loopRegion;
  if (!region) return;

  // Draw a line every 4 beats inside the loop region only.
  // beat 0 = region start, so lines at beat 4, 8, 12 (not at 0 or 16 — those are the handles).
  const totalBeats = region.beats;
  const dur = region.endSec - region.startSec;
  if (dur <= 0) return;

  const beatDur = dur / totalBeats;

  ctx.strokeStyle = '#ffffff40';
  ctx.lineWidth = 1;

  for (let b = 4; b < totalBeats; b += 4) {
    const t = region.startSec + b * beatDur;
    const x = secToPx(t);
    if (x < 0 || x > w) continue;
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, h);
    ctx.stroke();
  }
}

// ── Playhead ──────────────────────────────────────────────────

let rafId = 0;

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

function rafLoop() {
  drawWaveform();
  rafId = requestAnimationFrame(rafLoop);
}

// ── Drag interaction ──────────────────────────────────────────

type DragHandle = 'start' | 'end' | 'body' | 'pan' | 'new' | null;
const dragging = ref<DragHandle>(null);
const dragOffsetSec = ref(0);
const panStartX = ref(0);
const panStartViewSec = ref(0);
let newRegionAnchorSec = 0; // start point for a brand-new selection drag

function hitTest(px: number): DragHandle {
  const region = props.loopRegion;
  if (!region) return null;
  const sx = secToPx(region.startSec);
  const ex = secToPx(region.endSec);
  if (Math.abs(px - sx) < HANDLE_W + 4) return 'start';
  if (Math.abs(px - ex) < HANDLE_W + 4) return 'end';
  if (px > sx && px < ex) return 'body';
  return null;
}

function canvasPx(clientX: number): number {
  const canvas = canvasEl.value;
  if (!canvas) return 0;
  return clientX - canvas.getBoundingClientRect().left;
}

function onMouseDown(e: MouseEvent) {
  const px = canvasPx(e.clientX);
  const hit = hitTest(px);
  const locked = regionLocked.value;

  if (e.button === 2) {
    dragging.value = 'pan';
    panStartX.value = px;
    panStartViewSec.value = viewStartSec.value;
  } else if ((hit === 'start' || hit === 'end') && !locked) {
    dragging.value = hit;
  } else if (hit === 'body') {
    const region = props.loopRegion;
    if (!region) return;
    dragging.value = 'body';
    dragOffsetSec.value = pxToSec(px) - region.startSec;
  } else if (!locked) {
    dragging.value = 'new';
    newRegionAnchorSec = pxToSec(px);
  }

  if (dragging.value) {
    window.addEventListener('mousemove', onMouseMoveWindow);
    window.addEventListener('mouseup', onMouseUp);
  }
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

  if (dragging.value === 'new') {
    const sec = pxToSec(px);
    const start = Math.min(newRegionAnchorSec, sec);
    const end = Math.max(newRegionAnchorSec, sec);
    if (end - start > MIN_NEW_REGION_SEC) {
      emit('setRegion', { startSec: start, endSec: end, beats: props.loopBeats });
      dragging.value = sec < newRegionAnchorSec ? 'start' : 'end';
    }
    drawWaveform();
    return;
  }

  const region = props.loopRegion;
  if (!region) return;
  const newRegion: LoopRegion = { ...region };

  if (dragging.value === 'start') {
    newRegion.startSec = Math.max(0, Math.min(pxToSec(px), region.endSec - MIN_REGION_SEC));
  } else if (dragging.value === 'end') {
    newRegion.endSec = Math.min(
      trackDuration,
      Math.max(pxToSec(px), region.startSec + MIN_REGION_SEC)
    );
  } else if (dragging.value === 'body') {
    const dur = region.endSec - region.startSec;
    const newStart = Math.max(0, Math.min(pxToSec(px) - dragOffsetSec.value, trackDuration - dur));
    emit('moveRegion', newStart);
    drawWaveform();
    return;
  }

  emit('setRegion', newRegion);
  drawWaveform();
}

// Canvas hover — only updates cursor when not dragging
function onMouseMoveCanvas(e: MouseEvent) {
  if (dragging.value) return;
  const px = canvasPx(e.clientX);
  const hit = hitTest(px);
  const locked = regionLocked.value;
  const canvas = canvasEl.value;
  if (!canvas) return;
  canvas.style.cursor =
    hit === 'start' || hit === 'end'
      ? locked
        ? 'not-allowed'
        : 'ew-resize'
      : hit === 'body'
        ? 'grab'
        : locked
          ? 'default'
          : 'col-resize';
}

// Window-level move — fires even when mouse leaves canvas
function onMouseMoveWindow(e: MouseEvent) {
  if (!dragging.value) return;
  applyDrag(e.clientX);
}

function onWheel(e: WheelEvent) {
  const canvas = canvasEl.value;
  if (canvas) {
    const rect = canvas.getBoundingClientRect();
    const frac = (e.clientX - rect.left) / rect.width;
    const anchorSec = viewStartSec.value + frac * (viewEndSec.value - viewStartSec.value);
    if (e.deltaY < 0) zoomIn(anchorSec);
    else zoomOut(anchorSec);
  } else {
    if (e.deltaY < 0) zoomIn();
    else zoomOut();
  }
}

function onMouseUp() {
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
  () => props.buffer,
  (buf) => {
    if (buf) {
      trackDuration = buf.duration;
      buildFullPeaks();
      zoomIdx.value = ZOOM_LEVELS_SEC.indexOf(DEFAULT_ZOOM_SEC);
      const dur = viewDurationSec();
      viewStartSec.value = 0;
      viewEndSec.value = Math.min(dur, trackDuration);
    } else {
      rawChannel = null;
      sampleRate = 0;
      trackDuration = 0;
      visiblePeaks = null;
    }
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
  outline: 2px dashed #555;
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

.waveform__drop-icon {
  font-size: 3rem;
  color: #333;
}

.waveform__drop-hint {
  font-size: 0.75rem;
  letter-spacing: 0.15em;
  margin: 0;
  color: #555;
}

.waveform__drop-hint--sub {
  color: #333;
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

.waveform__ctrl-label {
  font-size: 0.6rem;
  letter-spacing: 0.2em;
  color: #555;
}

.waveform__beat-btn {
  background: #1a1a1a;
  border: 1px solid #2a2a2a;
  color: #777;
  font-family: var(--font-mono);
  font-size: 0.75rem;
  padding: 4px 12px;
  border-radius: 3px;
  cursor: pointer;
}

.waveform__beat-btn--active {
  border-color: v-bind(accent);
  color: v-bind(accent);
}

.waveform__lock-btn {
  background: #1a1a1a;
  border: 1px solid #2a2a2a;
  font-size: 0.75rem;
  padding: 4px 8px;
  border-radius: 3px;
  cursor: pointer;
  line-height: 1;
}

.waveform__lock-btn:hover {
  border-color: #555;
}

.waveform__lock-btn--unlocked {
  border-color: v-bind(accent);
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
