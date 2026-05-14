<template>
  <div class="overview">
    <canvas ref="canvasEl" class="overview__canvas" @mousedown="onMouseDown" @contextmenu.prevent />
    <div class="overview__times">
      <span ref="elapsedEl">0:00</span>
      <span ref="remainingEl">-0:00</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch } from 'vue';
import type { TrackData } from '@renderer/stores/decks';
import { buildWaveformImageData } from '@renderer/utils/waveformImage';

const props = defineProps<{
  accent: string;
  trackData: TrackData | null;
  getPlayheadPosition: () => number;
  fullSpectralData: Float32Array | null;
  loopRegion: { startSec: number; endSec: number } | null;
  loopActive: boolean;
  cuePoint: number;
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

const OVERVIEW_AMP_SCALE = 0.85;

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

  const peaks = props.fullSpectralData;
  if (!peaks) {
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = '#0a0a0a';
    ctx.fillRect(0, 0, w, h);
    return;
  }

  const cw = canvas.width;
  const ch = canvas.height;
  if (waveImgForPeaks !== peaks || waveImgForCw !== cw || waveImgForCh !== ch) {
    waveImgData = buildWaveformImageData(cw, ch, peaks, OVERVIEW_AMP_SCALE);
    waveImgForPeaks = peaks;
    waveImgForCw = cw;
    waveImgForCh = ch;
  }
  if (!waveImgData) return;
  ctx.putImageData(waveImgData, 0, 0);

  const region = props.loopRegion;
  if (region && trackDuration > 0) {
    const x1 = Math.max(0, (region.startSec / trackDuration) * w);
    const x2 = Math.min(w, (region.endSec / trackDuration) * w);
    if (x2 > x1) {
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
    const cueX = (props.cuePoint / trackDuration) * w;
    const TRI_W = 5;
    const TRI_H = 8;
    ctx.save();
    ctx.fillStyle = '#eab308';
    ctx.globalAlpha = 0.9;
    ctx.beginPath();
    ctx.moveTo(cueX - TRI_W, h);
    ctx.lineTo(cueX + TRI_W, h);
    ctx.lineTo(cueX, h - TRI_H);
    ctx.closePath();
    ctx.fill();
    ctx.restore();
  }

  const pos = props.getPlayheadPosition();
  const posRatio = trackDuration > 0 ? Math.min(1, pos / trackDuration) : 0;
  const px = posRatio * w;

  ctx.strokeStyle = '#ffffff';
  ctx.lineWidth = 1.5;
  ctx.globalAlpha = 0.85;
  ctx.beginPath();
  ctx.moveTo(px, 0);
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
  return dragRectWidth > 0 ? (px / dragRectWidth) * trackDuration : 0;
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

function onMouseUp(e: MouseEvent) {
  isDragging = false;
  window.removeEventListener('mousemove', onMouseMoveWindow);
  window.removeEventListener('mouseup', onMouseUp);
  const px = e.clientX - dragRectLeft;
  emit('seek', Math.max(0, Math.min(pxToSec(px), trackDuration)));
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
  display: flex;
  flex-direction: column;
  gap: 0.25em;
  flex-shrink: 0;
}

.overview__canvas {
  width: 100%;
  height: 2.5em;
  display: block;
}

.overview__times {
  display: flex;
  justify-content: space-between;
  font-size: 0.6em;
  color: var(--color-muted);
  letter-spacing: 0.05em;
  font-variant-numeric: tabular-nums;
  padding: 0 0.2em;
}
</style>
