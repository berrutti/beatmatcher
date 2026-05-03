<template>
  <div class="lissajous-wrapper">
    <canvas ref="canvasEl" class="lissajous" />
    <p class="lissajous-hint">phase scope</p>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { computeDotPosition, segmentAlpha } from './lissajousScope';

type PhaseSource = {
  getPhase: () => number;
  accent: string;
  label?: string;
};

const props = defineProps<{
  sources: PhaseSource[];
}>();

const DOT_RADIUS = 4;
const RING_RADIUS_RATIO = 0.82;
const HISTORY_SIZE = 90;
const FADE_DECAY = 0.05;
const GLOW_THRESHOLD = 0.7;
const GLOW_BLUR_MAX = 12;

const canvasEl = ref<HTMLCanvasElement | null>(null);
let rafId = 0;
let history: Array<[number, number]> = [];
let wasPlaying = false;
let fadeFactor = 0;

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
  const half = Math.min(w, h) / 2;
  const amplitude = half * RING_RADIUS_RATIO;
  const cx = w / 2;
  const cy = h / 2;
  const phases = props.sources.map((s) => s.getPhase());
  const anyPlaying = phases.some((p) => p !== 0);

  ctx.fillStyle = '#0a0a0a';
  ctx.fillRect(0, 0, w, h);

  if (anyPlaying) {
    if (!wasPlaying) history = [];
    wasPlaying = true;
    fadeFactor = 1;
    const point = computeDotPosition(phases, amplitude, cx, cy);
    history.push(point);
    if (history.length > HISTORY_SIZE) history.shift();
  } else if (wasPlaying) {
    fadeFactor -= FADE_DECAY;
    if (fadeFactor <= 0) {
      history = [];
      wasPlaying = false;
      fadeFactor = 0;
    }
  }

  if (history.length > 1) {
    for (let i = 1; i < history.length; i++) {
      const t = i / (history.length - 1);
      ctx.globalAlpha = segmentAlpha(i, history.length, fadeFactor);
      ctx.beginPath();
      ctx.moveTo(history[i - 1][0], history[i - 1][1]);
      ctx.lineTo(history[i][0], history[i][1]);
      ctx.strokeStyle = '#ffffff';
      ctx.lineWidth = DOT_RADIUS * 2;
      ctx.lineCap = 'round';
      ctx.shadowColor = t > GLOW_THRESHOLD ? '#ffffff' : 'transparent';
      ctx.shadowBlur = t > GLOW_THRESHOLD ? GLOW_BLUR_MAX * t : 0;
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    ctx.shadowBlur = 0;
    ctx.shadowColor = 'transparent';
  }

  rafId = requestAnimationFrame(draw);
}

onMounted(() => {
  const canvas = canvasEl.value;
  if (!canvas) return;
  requestAnimationFrame(() => {
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (!w || !h) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = w * dpr;
    canvas.height = h * dpr;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    history = [];
    wasPlaying = false;
    fadeFactor = 0;
    ctx.fillStyle = '#0a0a0a';
    ctx.fillRect(0, 0, w, h);
    rafId = requestAnimationFrame(draw);
  });
});

onUnmounted(() => {
  cancelAnimationFrame(rafId);
});
</script>

<style scoped>
.lissajous-wrapper {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  width: 100%;
  height: 100%;
}

.lissajous {
  display: block;
  width: 100%;
  flex: 1;
  min-height: 0;
  border-radius: 4px;
  border: 1px solid #2a2a2a;
}

.lissajous-hint {
  font-size: 0.6rem;
  color: #444;
  letter-spacing: 0.15em;
  margin: 0;
  text-transform: uppercase;
}
</style>
