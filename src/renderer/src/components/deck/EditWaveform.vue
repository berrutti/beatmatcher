<template>
  <div class="waveform" :style="{ '--accent': props.accent }">
    <div v-show="!props.trackData && !props.denseSpectralData" class="waveform__empty">
      <span class="waveform__empty-text">{{ $t(emptyTextKey) }}</span>
    </div>

    <div v-show="props.trackData || props.denseSpectralData" class="waveform__content">
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
          <button class="waveform__zoom-btn" tabindex="-1" @click="() => zoomOut()">−</button>
          <span class="waveform__zoom-label">{{ zoomLabel }}</span>
          <button class="waveform__zoom-btn" tabindex="-1" @click="() => zoomIn()">+</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// Rendering approach inspired by Mixxx (https://github.com/mixxxdj/mixxx):
// mean-in-window pixel aggregation with an overscan bitmap cache for
// stable, LOD-aware display across all zoom levels.
import { ref, onMounted, onUnmounted, watch, computed } from 'vue';
import type { TrackData } from '@renderer/stores/decks';
import {
  waveformColumns,
  waveformImageData,
  type WaveformPaint
} from '@renderer/utils/waveformImage';
import { beatGridStep, beatMarkerKind, visibleBeats } from '@renderer/utils/beatGrid';
import {
  MIN_WAVEFORM_BEAT_SPACING_PX,
  MIN_WAVEFORM_BAR_SPACING_PX,
  EDIT_GRID,
  drawBeatLine,
  drawBarLine,
  drawBeatMarker
} from '@renderer/utils/beatGridDraw';
import { loopRegionRect } from '@renderer/utils/loopRegionRect';
import { wheelIntent } from '@renderer/utils/wheelIntent';
import { snappedToDevicePixel } from '@renderer/utils/stripGeometry';
import { sameBandBalance, type BandSquares } from '@renderer/utils/bandBalance';
import {
  anchoredView,
  clampedView,
  visibleSpanLabel,
  type SecondsView
} from '@renderer/utils/waveformZoom';
import { DECK_LOD_DEBOUNCE_MS } from '@renderer/utils/waveformLod';
import { drawCueTriangle } from '@renderer/utils/cueMarker';
import { waveformPalette } from '@renderer/utils/waveformPalettes';
import { EDIT_SCALES } from '@renderer/utils/waveformPaints';
import { DEFAULT_METER, type WaveformStyleOption } from '@renderer/utils/types';
import type {
  BitmapRange,
  BuiltBitmap,
  CacheSource,
  PeakCache
} from '@renderer/utils/waveformCache';
import {
  overscanRange,
  cacheSource,
  bitmapRange,
  bitmapPointRange,
  bitmapIsStale,
  builtFromSameSource,
  pointsComplete
} from '@renderer/utils/waveformCache';

const props = defineProps<{
  accent: string;
  trackData: TrackData | null;
  loading: boolean;
  trackBpm: number | null;
  beatOffset: number;
  cuePoint: number;
  loopRegion: { startSec: number; endSec: number } | null;
  loopActive: boolean;
  denseSpectralData: Float32Array | null;
  denseSpectralRate: number;
  densePointsReady: number;
  bandsReady: boolean;
  bandBalance: [number, number, number];
  bandReference: number;
  waveformStyle: WaveformStyleOption;
  getTrackPosition: () => number | null;
  getPlayheadPosition: () => number;
  getSpectralWaveformRegion: (
    startSec: number,
    endSec: number,
    numPoints: number
  ) => Promise<ArrayBuffer>;
}>();

// A track on its way in is not an empty deck: the strip stays blank until the
// decode lands, which is long enough to read as nothing having happened.
const emptyTextKey = computed(() =>
  props.loading ? 'editWaveform.loading' : 'editWaveform.noTrackLoaded'
);

const emit = defineEmits<{
  setBeatOffset: [sec: number];
  seek: [sec: number];
}>();

function editPaint(style: WaveformStyleOption, balance: BandSquares): WaveformPaint {
  return { ...EDIT_SCALES, palette: waveformPalette(style, balance) };
}
const PLAYHEAD_LINE_WIDTH = 1.5;
const PLAYHEAD_ALPHA = 0.9;
const PLAYHEAD_ARROW_HALF = 5;
const PLAYHEAD_ARROW_HEIGHT = 8;

const ZOOM_LEVELS_SEC = [0.25, 0.5, 1, 2, 5, 10, 20, 30, 60, 120, 300];
const DEFAULT_ZOOM_SEC = 10;

const canvasEl = ref<HTMLCanvasElement | null>(null);

const trackDuration = ref(0);

// Plain vars, not refs: only the rAF draw loop reads them, so reactivity would
// cost a notification per mousemove for nothing.
let viewStartSec = 0;
let viewEndSec = 0;

const zoomIdx = ref(ZOOM_LEVELS_SEC.indexOf(DEFAULT_ZOOM_SEC));

const zoomLabel = computed(() =>
  visibleSpanLabel(ZOOM_LEVELS_SEC[zoomIdx.value], trackDuration.value)
);

function viewDurationSec(): number {
  return ZOOM_LEVELS_SEC[zoomIdx.value];
}

function showView(view: SecondsView) {
  viewStartSec = view.startSec;
  viewEndSec = view.endSec;
}

function viewCentreSec(): number {
  return (viewStartSec + viewEndSec) / 2;
}

function setZoom(idx: number, anchorSec: number, anchorFrac: number) {
  const newZoom = Math.max(0, Math.min(ZOOM_LEVELS_SEC.length - 1, idx));
  if (newZoom === zoomIdx.value) return;
  zoomIdx.value = newZoom;
  showView(anchoredView(anchorSec, anchorFrac, viewDurationSec(), trackDuration.value));
  ensurePeaks();
}

function zoomIn(anchorSec: number = viewCentreSec(), anchorFrac = 0.5) {
  setZoom(zoomIdx.value - 1, anchorSec, anchorFrac);
}
function zoomOut(anchorSec: number = viewCentreSec(), anchorFrac = 0.5) {
  setZoom(zoomIdx.value + 1, anchorSec, anchorFrac);
}

const OVERSCAN_FACTOR = 1.0;
const MAX_BITMAP_PX = 8192;
const MIN_FETCH_POINTS = 64;

type PeaksCache = PeakCache & { peaks: Float32Array };
let cache: PeaksCache | null = null;

let waveImgBitmap: ImageBitmap | null = null;
let bitmapForStyle: WaveformStyleOption | null = null;
let builtBalance: BandSquares | null = null;
let builtReference = -1;
let builtBitmap: BuiltBitmap | null = null;
let bitmapBuildInFlight = false;

function requiredPtsPerSec(): number {
  const canvas = canvasEl.value;
  const zoomSec = viewEndSec - viewStartSec;
  if (!canvas || zoomSec <= 0) return 0;
  // Physical pixels, so a Retina zoom needing more than the dense LOD holds falls
  // through to a fetch rather than serving it blurred.
  return canvas.width / zoomSec;
}

function peaksSource(): CacheSource {
  return cacheSource(
    cache,
    viewStartSec,
    viewEndSec,
    requiredPtsPerSec(),
    props.denseSpectralRate,
    pointsComplete(props.denseSpectralData, props.densePointsReady)
  );
}

function servedLocally(): boolean {
  const source = peaksSource();
  return source === 'keep' || (source === 'dense' && sliceFromDense());
}

// A cache that moved with the pan would discard the bitmap being built for it.
function sliceFromDense(): boolean {
  const dense = props.denseSpectralData;
  const denseRate = props.denseSpectralRate;
  const ready = props.densePointsReady;
  if (!dense || denseRate <= 0 || ready <= 0) return false;

  cache = {
    peaks: dense.subarray(0, ready * 4),
    startSec: 0,
    endSec: ready / denseRate,
    ptsPerSec: denseRate
  };
  return true;
}

let isFetching = false;
let pendingFetch = false;
let fetchDebounceTimer = 0;
let isUnmounted = false;

async function fetchPeaksForView() {
  if (!props.trackData) return;
  isFetching = true;
  const { startSec, endSec } = overscanRange(
    viewStartSec,
    viewEndSec,
    trackDuration.value,
    OVERSCAN_FACTOR
  );
  const rate = requiredPtsPerSec();
  if (rate <= 0 || endSec <= startSec) {
    isFetching = false;
    return;
  }
  const numPoints = Math.max(MIN_FETCH_POINTS, Math.ceil((endSec - startSec) * rate));
  try {
    const result = await props.getSpectralWaveformRegion(startSec, endSec, numPoints);
    const peaks = new Float32Array(result);
    // A deck still reducing its bands answers short, and caching that would cover the view
    // with a picture nothing would ask to replace.
    const answered = peaks.length >= numPoints * 4;
    // Only apply if this fetch still covers the current view. A later
    // pan/zoom might have moved us outside the fetched range.
    if (answered && startSec <= viewStartSec + 1e-6 && endSec >= viewEndSec - 1e-6) {
      cache = { peaks, startSec, endSec, ptsPerSec: rate };
    }
  } catch (err) {
    console.error('[WaveformDisplay] spectral fetch failed:', err);
  }
  isFetching = false;
  if (pendingFetch) {
    pendingFetch = false;
    // Re-check: dense LOD may have arrived, or the view may have moved
    // back inside the current cache. In either case we skip the IPC.
    if (!servedLocally()) {
      fetchPeaksForView();
    }
  }
}

function ensurePeaks() {
  if (isUnmounted) return;
  if (servedLocally()) return;
  if (props.denseSpectralRate <= 0) return;
  if (isFetching) {
    pendingFetch = true;
    return;
  }
  clearTimeout(fetchDebounceTimer);
  fetchDebounceTimer = window.setTimeout(() => {
    if (servedLocally()) return;
    fetchPeaksForView();
  }, DECK_LOD_DEBOUNCE_MS);
}

async function renderBitmap(
  peaks: Float32Array,
  builtFrom: PeakCache,
  range: BitmapRange,
  canvasH: number
) {
  try {
    const totalPoints = (peaks.length / 4) | 0;
    const { startIndex, endIndex } = bitmapPointRange(builtFrom, range, totalPoints);
    const columns = waveformColumns(peaks, range.width, startIndex, endIndex, props.bandReference);
    const style = props.waveformStyle;
    const balance: BandSquares = [...props.bandBalance];
    const imgData = waveformImageData(range.width, canvasH, columns, editPaint(style, balance));
    const bmp = await createImageBitmap(imgData);
    if (builtFromSameSource(peaks, cache?.peaks ?? null)) {
      waveImgBitmap = bmp;
      bitmapForStyle = style;
      builtBalance = balance;
      builtReference = props.bandReference;
      builtBitmap = {
        startSec: range.startSec,
        endSec: range.endSec,
        width: range.width,
        canvasHeight: canvasH
      };
    }
  } catch {
    // createImageBitmap failure. Skip this bitmap update
  } finally {
    bitmapBuildInFlight = false;
  }
}

function ensureBitmap(canvasH: number) {
  if (!cache || canvasH <= 0) return;
  const samePaint =
    bitmapForStyle === props.waveformStyle &&
    builtBalance !== null &&
    sameBandBalance(builtBalance, props.bandBalance) &&
    builtReference === props.bandReference;
  if (
    samePaint &&
    !bitmapIsStale(builtBitmap, cache, viewStartSec, viewEndSec, canvasH, MAX_BITMAP_PX)
  )
    return;
  if (bitmapBuildInFlight) return;

  const range = bitmapRange(cache, viewStartSec, viewEndSec, MAX_BITMAP_PX);
  if (!range) return;

  bitmapBuildInFlight = true;
  renderBitmap(cache.peaks, cache, range, canvasH);
}

function pxToSec(localX: number): number {
  const canvas = canvasEl.value;
  if (!canvas) return 0;
  // A collapsed canvas divides by zero, and localX 0 makes that NaN rather than
  // Infinity, which the caller's clamp cannot catch: it would reach `seek`.
  if (canvas.clientWidth <= 0) return viewStartSec;
  return viewStartSec + (localX / canvas.clientWidth) * (viewEndSec - viewStartSec);
}

// A bitmap edge or a line on half a device pixel is resampled against its neighbour, which
// crawls as the view moves.
function secToPx(sec: number): number {
  const canvas = canvasEl.value;
  if (!canvas) return 0;
  const span = viewEndSec - viewStartSec;
  if (span <= 0) return 0;
  const px = ((sec - viewStartSec) / span) * canvas.clientWidth;
  return snappedToDevicePixel(px, window.devicePixelRatio || 1);
}

function drawWaveform() {
  const canvas = canvasEl.value;
  if (!canvas || trackDuration.value <= 0) return;

  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  const dpr = window.devicePixelRatio || 1;
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  if (width === 0 || height === 0) return;

  if (canvas.width !== width * dpr || canvas.height !== height * dpr) {
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    ctx.scale(dpr, dpr);
  }

  ensureBitmap(canvas.height);

  ctx.fillStyle = '#0a0a0a';
  ctx.fillRect(0, 0, width, height);
  if (waveImgBitmap && builtBitmap) {
    const bitmapLeftPx = secToPx(builtBitmap.startSec);
    const bitmapRightPx = secToPx(builtBitmap.endSec);
    const bitmapWidthPx = bitmapRightPx - bitmapLeftPx;
    if (bitmapWidthPx > 0) {
      ctx.drawImage(waveImgBitmap, bitmapLeftPx, 0, bitmapWidthPx, height);
    }
  }

  drawLoopRegion(ctx, width, height);
  drawRuler(ctx, width, height);
  drawDownbeatMarker(ctx, width, height);
  drawCueMarker(ctx, width, height);
  drawPlayhead(ctx, width, height);
}

function drawLoopRegion(ctx: CanvasRenderingContext2D, width: number, height: number) {
  const rect = loopRegionRect(secToPx, props.loopRegion, width);
  if (!rect) return;
  const { startX, endX } = rect;
  ctx.save();
  ctx.fillStyle = props.loopActive ? '#ca8a04' : '#78716c';
  ctx.globalAlpha = 0.25;
  ctx.fillRect(startX, 0, endX - startX, height);
  ctx.globalAlpha = props.loopActive ? 0.7 : 0.35;
  ctx.strokeStyle = props.loopActive ? '#ca8a04' : '#78716c';
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.moveTo(startX, 0);
  ctx.lineTo(startX, height);
  ctx.moveTo(endX, 0);
  ctx.lineTo(endX, height);
  ctx.stroke();
  ctx.restore();
}

// Not the meter: how fast the ruler thins as the view widens. Halves rather than quarters,
// or a jump of four leaves one zoom dense and the next sparse.
const BEATS_PER_STEP = 2;

function drawRuler(ctx: CanvasRenderingContext2D, width: number, height: number) {
  if (!props.trackBpm || props.trackBpm <= 0) return;
  const viewSpan = viewEndSec - viewStartSec;
  const pxPerBeat = ((60 / props.trackBpm) * width) / viewSpan;
  const step = beatGridStep(
    pxPerBeat,
    BEATS_PER_STEP,
    MIN_WAVEFORM_BEAT_SPACING_PX,
    MIN_WAVEFORM_BAR_SPACING_PX
  );
  if (pxPerBeat * step < 1) return;

  const dpr = window.devicePixelRatio || 1;
  for (const beat of visibleBeats(
    props.trackBpm,
    props.beatOffset,
    viewStartSec,
    viewEndSec,
    step,
    DEFAULT_METER
  )) {
    const beatX = secToPx(beat.sec);
    if (beatX < 0 || beatX > width) continue;
    if (beat.isDownbeat) {
      drawBarLine(ctx, beatX, 0, height, dpr, EDIT_GRID);
    } else {
      drawBeatLine(ctx, beatX, 0, height, dpr, EDIT_GRID);
    }
    drawBeatMarker(ctx, beatX, 0, height, beatMarkerKind(beat, DEFAULT_METER), EDIT_GRID);
  }
}

let rafId = 0;
let lastZoomTime = 0;
const ZOOM_COOLDOWN_MS = 150;

function drawPlayhead(ctx: CanvasRenderingContext2D, width: number, height: number) {
  const sec = props.getPlayheadPosition();
  const playheadX = secToPx(sec);
  if (playheadX < 0 || playheadX > width) return;
  const isPlaying = props.getTrackPosition() !== null;
  const color = isPlaying ? '#ffffff' : '#ef4444';
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = PLAYHEAD_LINE_WIDTH;
  ctx.globalAlpha = PLAYHEAD_ALPHA;
  ctx.beginPath();
  ctx.moveTo(playheadX, 0);
  ctx.lineTo(playheadX, height);
  ctx.stroke();
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.moveTo(playheadX - PLAYHEAD_ARROW_HALF, 0);
  ctx.lineTo(playheadX + PLAYHEAD_ARROW_HALF, 0);
  ctx.lineTo(playheadX, PLAYHEAD_ARROW_HEIGHT);
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

const MARKER_TRI_W = 7;
const MARKER_TRI_H = 11;
const MARKER_LINE_WIDTH = 1.5;

function drawDownbeatMarker(ctx: CanvasRenderingContext2D, width: number, height: number) {
  const markerX = secToPx(props.beatOffset);
  if (markerX < -MARKER_TRI_W || markerX > width + MARKER_TRI_W) return;
  ctx.save();
  ctx.strokeStyle = '#ffffff';
  ctx.fillStyle = '#ffffff';
  ctx.globalAlpha = 0.85;
  ctx.lineWidth = MARKER_LINE_WIDTH;
  ctx.beginPath();
  ctx.moveTo(markerX, 0);
  ctx.lineTo(markerX, height);
  ctx.stroke();
  ctx.beginPath();
  ctx.moveTo(markerX - MARKER_TRI_W, 0);
  ctx.lineTo(markerX + MARKER_TRI_W, 0);
  ctx.lineTo(markerX, MARKER_TRI_H);
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

function drawCueMarker(ctx: CanvasRenderingContext2D, width: number, height: number) {
  const markerX = secToPx(props.cuePoint);
  if (markerX < -MARKER_TRI_W || markerX > width + MARKER_TRI_W) return;
  drawCueTriangle(ctx, markerX, height, MARKER_TRI_W, -MARKER_TRI_H);
}

function rafLoop() {
  applyPendingDrag();
  // Sync only: an IPC fetch here would flood the backend during a fast drag.
  servedLocally();
  drawWaveform();
  rafId = requestAnimationFrame(rafLoop);
}

// mousemove is batched to rAF: only the latest clientX is stored per frame.
// Avoids redundant work when mousemove fires faster than display refresh (trackpads at 120 Hz).
let dragging: 'seek' | 'pan' | null = null;
let panStartX = 0;
let panStartViewSec = 0;
let dragRectLeft = 0;
let dragRectWidth = 0;
let pendingDragX: number | null = null;
function onMouseDown(event: MouseEvent) {
  const canvas = canvasEl.value;
  if (!canvas || !props.trackData) return;
  const rect = canvas.getBoundingClientRect();
  dragRectLeft = rect.left;
  dragRectWidth = rect.width;
  const localX = event.clientX - dragRectLeft;

  if (event.button === 2) {
    dragging = 'pan';
    panStartX = localX;
    panStartViewSec = viewStartSec;
  } else {
    dragging = 'seek';
    pendingDragX = event.clientX;
  }

  window.addEventListener('mousemove', onMouseMoveWindow);
  window.addEventListener('mouseup', onMouseUp);
}

function applyPendingDrag() {
  if (pendingDragX === null) return;
  const localX = pendingDragX - dragRectLeft;

  if (dragging === 'pan' && dragRectWidth > 0) {
    const viewSpan = viewEndSec - viewStartSec;
    const deltaSec = -((localX - panStartX) / dragRectWidth) * viewSpan;
    showView(clampedView(panStartViewSec + deltaSec, viewSpan, trackDuration.value));
  } else if (dragging === 'seek') {
    emit('seek', Math.max(0, Math.min(pxToSec(localX), trackDuration.value)));
  }

  pendingDragX = null;
}

function onMouseMoveWindow(event: MouseEvent) {
  if (!dragging) return;
  pendingDragX = event.clientX;
}

function onWheel(event: WheelEvent) {
  const canvas = canvasEl.value;
  if (!canvas) return;

  const rect = canvas.getBoundingClientRect();
  const frac = (event.clientX - rect.left) / rect.width;
  const viewSpan = viewEndSec - viewStartSec;

  if (wheelIntent(event) === 'pan') {
    const deltaSec = ((event.deltaX || event.deltaY) / rect.width) * viewSpan;
    showView(clampedView(viewStartSec + deltaSec, viewSpan, trackDuration.value));
    ensurePeaks();
    return;
  }

  if (event.deltaY === 0) return;
  const now = Date.now();
  if (now - lastZoomTime <= ZOOM_COOLDOWN_MS) return;
  lastZoomTime = now;
  const anchorSec = viewStartSec + frac * viewSpan;
  const anchorFrac = Math.max(0, Math.min(1, frac));
  if (event.deltaY < 0) zoomIn(anchorSec, anchorFrac);
  else zoomOut(anchorSec, anchorFrac);
}

function onMouseUp() {
  pendingDragX = null;
  const wasPan = dragging === 'pan';
  dragging = null;
  window.removeEventListener('mousemove', onMouseMoveWindow);
  window.removeEventListener('mouseup', onMouseUp);
  if (wasPan) {
    ensurePeaks();
  }
}

let resizeObserver: ResizeObserver | null = null;

watch(canvasEl, (element) => {
  if (element && !resizeObserver && element.parentElement) {
    resizeObserver = new ResizeObserver(() => {
      ensurePeaks();
    });
    resizeObserver.observe(element.parentElement);
  }
});

onMounted(() => {
  rafId = requestAnimationFrame(rafLoop);
});

onUnmounted(() => {
  isUnmounted = true;
  resizeObserver?.disconnect();
  cancelAnimationFrame(rafId);
  clearTimeout(fetchDebounceTimer);
  window.removeEventListener('mousemove', onMouseMoveWindow);
  window.removeEventListener('mouseup', onMouseUp);
});

function frameView(duration: number) {
  trackDuration.value = duration;
  zoomIdx.value = ZOOM_LEVELS_SEC.indexOf(DEFAULT_ZOOM_SEC);
  showView(clampedView(0, viewDurationSec(), duration));
  cache = null;
  builtBitmap = null;
  waveImgBitmap = null;
  ensurePeaks();
}

watch(
  () => props.trackData,
  (data) => {
    if (!data) {
      trackDuration.value = 0;
      cache = null;
      builtBitmap = null;
      waveImgBitmap = null;
      return;
    }
    // The points already framed the view unless the container hid its length, and the user
    // may have zoomed into it while the rest of the track was decoding.
    if (trackDuration.value <= 0) {
      frameView(data.duration);
      return;
    }
    trackDuration.value = data.duration;
    ensurePeaks();
  },
  { immediate: true }
);

// The reduction sizes its buffer from the whole track, so the length is known a chunk in
// rather than when the decode returns.
watch(
  () => props.denseSpectralData,
  (data) => {
    if (!data || props.denseSpectralRate <= 0) return;
    if (trackDuration.value <= 0) frameView(data.length / 4 / props.denseSpectralRate);
    else ensurePeaks();
  },
  // Immediate, or a view opened onto a load already in flight waits for the decode.
  { immediate: true }
);

// The reduction hands over its points a chunk at a time, so the view takes each as it lands.
watch(
  () => props.densePointsReady,
  () => ensurePeaks()
);

// Points complete a moment before the bands they were reduced from are stored, so this is
// the retry for a region asked for inside that gap.
watch(
  () => props.bandsReady,
  (ready) => {
    if (ready) ensurePeaks();
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
  overflow: hidden;
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
  font-size: 0.7em;
  letter-spacing: 0.02em;
  opacity: 0.6;
}

.waveform__content {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.waveform__canvas {
  flex: 1;
  min-height: 0;
  width: 100%;
  display: block;
  cursor: crosshair;
}

.waveform__controls {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px;
  border-top: 1px solid #1e1e1e;
  background: #0d0d0d;
}

.waveform__bpm-readout {
  font-size: 0.85em;
  font-weight: 700;
  color: var(--accent);
  margin-left: auto;
  letter-spacing: 0.02em;
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
  font-family: var(--font);
  font-size: 1em;
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
  font-size: 0.65em;
  letter-spacing: 0.04em;
  color: #555;
  min-width: 24px;
  text-align: center;
}
</style>
