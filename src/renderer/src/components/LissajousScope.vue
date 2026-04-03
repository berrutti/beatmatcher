<template>
  <div class="lissajous-wrapper">
    <div class="lissajous-labels">
      <span
        v-for="(source, i) in sources"
        :key="i"
        class="lissajous-label"
        :style="{ color: source.accent }"
        >{{ source.label }}</span
      >
    </div>
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

const SIZE = 200;
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
  const half = SIZE / 2;
  const amplitude = half * RING_RADIUS_RATIO;
  const phases = props.sources.map((s) => s.getPhase());
  const anyPlaying = phases.some((p) => p !== 0);

  ctx.fillStyle = '#0a0a0a';
  ctx.fillRect(0, 0, SIZE, SIZE);

  if (anyPlaying) {
    if (!wasPlaying) history = [];
    wasPlaying = true;
    fadeFactor = 1;

    const [x, y] = computeDotPosition(phases, amplitude, half);
    history.push([x, y]);
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
  const dpr = window.devicePixelRatio || 1;
  canvas.width = SIZE * dpr;
  canvas.height = SIZE * dpr;
  canvas.style.width = `${SIZE}px`;
  canvas.style.height = `${SIZE}px`;
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.scale(dpr, dpr);

  ctx.fillStyle = '#0a0a0a';
  ctx.fillRect(0, 0, SIZE, SIZE);

  rafId = requestAnimationFrame(draw);
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
  gap: 8px;
}

.lissajous-labels {
  display: flex;
  gap: 12px;
}

.lissajous-label {
  font-size: 0.7rem;
  letter-spacing: 0.2em;
  font-weight: 700;
}

.lissajous {
  display: block;
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
