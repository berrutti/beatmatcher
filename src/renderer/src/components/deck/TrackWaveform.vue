<template>
  <div class="overview">
    <div class="overview__times">
      <span ref="elapsedEl" class="overview__time overview__time--elapsed">0:00</span>
      <span ref="remainingEl" class="overview__time overview__time--remaining">-0:00</span>
    </div>
    <canvas ref="canvasEl" class="overview__canvas" @mousedown="onMouseDown" @contextmenu.prevent />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch } from 'vue';
import type { TrackData } from '@renderer/stores/decks';
import {
  waveformColumns,
  waveformImageData,
  maxBarTop,
  type WaveformPaint
} from '@renderer/utils/waveformImage';
import { loopRegionRect } from '@renderer/utils/loopRegionRect';
import { waveformPalette } from '@renderer/utils/waveformPalettes';
import { OVERVIEW_SCALES } from '@renderer/utils/waveformPaints';
import type { WaveformStyleOption } from '@renderer/utils/types';
import { drawCueTriangle } from '@renderer/utils/cueMarker';

const props = defineProps<{
  accent: string;
  trackData: TrackData | null;
  getPlayheadPosition: () => number;
  denseSpectralData: Float32Array | null;
  densePointsReady: number;
  loopRegion: { startSec: number; endSec: number } | null;
  loopActive: boolean;
  cuePoint: number;
  bandBalance: [number, number, number];
  waveformStyle: WaveformStyleOption;
}>();

const emit = defineEmits<{ seek: [sec: number] }>();

const canvasEl = ref<HTMLCanvasElement | null>(null);
const elapsedEl = ref<HTMLSpanElement | null>(null);
const remainingEl = ref<HTMLSpanElement | null>(null);

let trackDuration = 0;
let rafId = 0;

let waveImgData: ImageData | null = null;
let waveImgForPeaks: Float32Array | null = null;
let waveImgForCw = 0;
let waveImgForCh = 0;
let waveImgForStyle: WaveformStyleOption | null = null;
let waveImgForReady = -1;
let playheadTop = 0;

function overviewPaint(): WaveformPaint {
  return {
    ...OVERVIEW_SCALES,
    palette: waveformPalette(props.waveformStyle, props.bandBalance)
  };
}
const CUE_TRIANGLE_WIDTH = 4;
const CUE_TRIANGLE_HEIGHT = 8;
// Half the cue triangle's width, reserved on both edges so the triangle
// never gets clipped when the cue point sits at the very start or end.
const SIDE_MARGIN = CUE_TRIANGLE_WIDTH;

function formatSec(sec: number): string {
  const abs = Math.abs(sec);
  const m = Math.floor(abs / 60);
  const s = Math.floor(abs % 60);
  return `${m}:${String(s).padStart(2, '0')}`;
}

function draw() {
  const canvas = canvasEl.value;
  if (!canvas) return;
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

  const peaks = props.denseSpectralData;
  if (!peaks) {
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = '#0a0a0a';
    ctx.fillRect(0, 0, w, h);
    return;
  }

  const marginPx = SIDE_MARGIN * dpr;
  const cw = canvas.width - marginPx * 2;
  const ch = canvas.height;
  if (
    waveImgForPeaks !== peaks ||
    waveImgForCw !== cw ||
    waveImgForCh !== ch ||
    waveImgForStyle !== props.waveformStyle ||
    waveImgForReady !== props.densePointsReady
  ) {
    const style = overviewPaint();
    const columns = waveformColumns(peaks, cw);
    waveImgData = waveformImageData(cw, ch, columns, style);
    playheadTop = maxBarTop(columns, ch, style);
    waveImgForPeaks = peaks;
    waveImgForCw = cw;
    waveImgForCh = ch;
    waveImgForStyle = props.waveformStyle;
    waveImgForReady = props.densePointsReady;
  }
  if (!waveImgData) return;
  ctx.clearRect(0, 0, w, h);
  ctx.putImageData(waveImgData, marginPx, 0);

  const usableW = w - SIDE_MARGIN * 2;
  if (trackDuration > 0) {
    const xFor = (sec: number) => SIDE_MARGIN + (sec / trackDuration) * usableW;
    const rect = loopRegionRect(xFor, props.loopRegion, w);
    if (rect) {
      const { startX: x1, endX: x2 } = rect;
      ctx.fillStyle = props.loopActive ? '#ca8a04' : '#78716c';
      ctx.globalAlpha = 0.3;
      ctx.fillRect(x1, 0, x2 - x1, h);
      ctx.globalAlpha = props.loopActive ? 0.7 : 0.4;
      ctx.strokeStyle = props.loopActive ? '#ca8a04' : '#78716c';
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.moveTo(x1, 0);
      ctx.lineTo(x1, h);
      ctx.moveTo(x2, 0);
      ctx.lineTo(x2, h);
      ctx.stroke();
      ctx.globalAlpha = 1;
    }
  }

  if (trackDuration > 0) {
    const cueX = SIDE_MARGIN + (props.cuePoint / trackDuration) * usableW;
    drawCueTriangle(ctx, cueX, h, CUE_TRIANGLE_WIDTH, -CUE_TRIANGLE_HEIGHT);
  }

  const pos = props.getPlayheadPosition();
  const posRatio = trackDuration > 0 ? Math.min(1, pos / trackDuration) : 0;
  const px = SIDE_MARGIN + posRatio * usableW;

  ctx.strokeStyle = '#ffffff';
  ctx.lineWidth = 1.5;
  ctx.globalAlpha = 0.85;
  ctx.beginPath();
  ctx.moveTo(px, playheadTop / dpr);
  ctx.lineTo(px, h);
  ctx.stroke();
  ctx.globalAlpha = 1;
}

function updateTimes() {
  if (!trackDuration) return;
  const pos = props.getPlayheadPosition();
  if (elapsedEl.value) elapsedEl.value.textContent = formatSec(pos);
  if (remainingEl.value)
    remainingEl.value.textContent = '-' + formatSec(Math.max(0, trackDuration - pos));
}

let dragRectLeft = 0;
let dragRectWidth = 0;
let isDragging = false;

function pxToSec(px: number): number {
  const usableWidth = dragRectWidth - SIDE_MARGIN * 2;
  return usableWidth > 0 ? ((px - SIDE_MARGIN) / usableWidth) * trackDuration : 0;
}

function onMouseDown(e: MouseEvent) {
  if (e.button !== 0) return;
  const canvas = canvasEl.value;
  if (!canvas || !trackDuration) return;
  const rect = canvas.getBoundingClientRect();
  dragRectLeft = rect.left;
  dragRectWidth = rect.width;
  isDragging = true;
  const px = e.clientX - dragRectLeft;
  emit('seek', Math.max(0, Math.min(pxToSec(px), trackDuration)));
  window.addEventListener('mousemove', onMouseMoveWindow);
  window.addEventListener('mouseup', onMouseUp);
}

function onMouseMoveWindow(e: MouseEvent) {
  if (!isDragging) return;
  const px = e.clientX - dragRectLeft;
  emit('seek', Math.max(0, Math.min(pxToSec(px), trackDuration)));
}

function onMouseUp() {
  isDragging = false;
  window.removeEventListener('mousemove', onMouseMoveWindow);
  window.removeEventListener('mouseup', onMouseUp);
}

function rafLoop() {
  draw();
  updateTimes();
  rafId = requestAnimationFrame(rafLoop);
}

onMounted(() => {
  rafId = requestAnimationFrame(rafLoop);
});

onUnmounted(() => {
  cancelAnimationFrame(rafId);
  window.removeEventListener('mousemove', onMouseMoveWindow);
  window.removeEventListener('mouseup', onMouseUp);
});

watch(
  () => props.trackData,
  (data) => {
    if (data) {
      trackDuration = data.duration;
    } else {
      trackDuration = 0;
    }
  },
  { immediate: true }
);
</script>

<style scoped>
.overview {
  width: 100%;
  flex-shrink: 0;
}

.overview__times {
  display: flex;
  justify-content: space-between;
  padding: 0 0.3em;
}

.overview__canvas {
  width: 100%;
  height: 2.5em;
  display: block;
}

.overview__time {
  font-size: 0.6em;
  color: var(--color-muted);
  letter-spacing: 0.02em;
  font-variant-numeric: tabular-nums;
  pointer-events: none;
}
</style>
