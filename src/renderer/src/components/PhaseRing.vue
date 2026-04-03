<template>
  <canvas ref="canvasEl" class="phase-ring" />
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import type { LoopRegion } from '@renderer/audio/LoopEngine';

const props = defineProps<{
  accent: string;
  loopRegion: LoopRegion | null;
  getTrackPosition: () => number | null;
}>();

const LINE_WIDTH_RATIO = 0.065;
const FLASH_DECAY = 0.82;
const BEAT_CROSSING_HIGH = 0.85;
const BEAT_CROSSING_LOW = 0.15;
const FLASH_SHADOW_BLUR = 24;
const DOT_EXTRA_RADIUS = 2;
const DOT_SHADOW_BASE = 10;
const DOT_SHADOW_PEAK = 20;

const canvasEl = ref<HTMLCanvasElement | null>(null);
let rafId = 0;
let prevBeatFrac = 0;
let flashStrength = 0;

function draw() {
  const canvas = canvasEl.value;
  if (!canvas) return;
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  const dpr = window.devicePixelRatio || 1;
  const SIZE = canvas.clientWidth;
  const LINE_WIDTH = SIZE * LINE_WIDTH_RATIO;
  const cx = SIZE / 2;
  const cy = SIZE / 2;
  const radius = SIZE / 2 - LINE_WIDTH;

  canvas.width = SIZE * dpr;
  canvas.height = SIZE * dpr;
  ctx.scale(dpr, dpr);
  ctx.clearRect(0, 0, SIZE, SIZE);

  ctx.beginPath();
  ctx.arc(cx, cy, radius, 0, Math.PI * 2);
  ctx.strokeStyle = '#2a2a2a';
  ctx.lineWidth = LINE_WIDTH;
  ctx.stroke();

  let phase4 = 0;
  let beatFrac = 0;
  let playing = false;

  if (props.loopRegion) {
    const positionSec = props.getTrackPosition();
    if (positionSec !== null) {
      playing = true;
      const loopDur = props.loopRegion.endSec - props.loopRegion.startSec;
      const beatDur = loopDur / props.loopRegion.beats;
      const posInLoop = positionSec - props.loopRegion.startSec;
      const currentBeat = posInLoop / beatDur;
      beatFrac = currentBeat % 1;
      phase4 = (currentBeat % 4) / 4;
    }
  }

  if (playing && prevBeatFrac > BEAT_CROSSING_HIGH && beatFrac < BEAT_CROSSING_LOW) {
    flashStrength = 1.0;
  }
  prevBeatFrac = beatFrac;

  if (flashStrength > 0.01) {
    flashStrength *= FLASH_DECAY;
  } else {
    flashStrength = 0;
  }

  const startAngle = -Math.PI / 2;
  const endAngle = startAngle + phase4 * Math.PI * 2;

  if (flashStrength > 0.01) {
    ctx.save();
    ctx.beginPath();
    ctx.arc(cx, cy, radius, startAngle, endAngle);
    ctx.strokeStyle = props.accent;
    ctx.lineWidth = LINE_WIDTH;
    ctx.shadowColor = props.accent;
    ctx.shadowBlur = FLASH_SHADOW_BLUR * flashStrength;
    ctx.globalAlpha = 0.4 + 0.6 * flashStrength;
    ctx.stroke();
    ctx.restore();
  }

  ctx.beginPath();
  ctx.arc(cx, cy, radius, startAngle, endAngle);
  ctx.strokeStyle = props.accent;
  ctx.lineWidth = LINE_WIDTH;
  ctx.shadowColor = 'transparent';
  ctx.shadowBlur = 0;
  ctx.globalAlpha = 1;
  ctx.stroke();

  const dotX = cx + radius * Math.cos(endAngle);
  const dotY = cy + radius * Math.sin(endAngle);
  ctx.beginPath();
  ctx.arc(dotX, dotY, LINE_WIDTH / 2 + DOT_EXTRA_RADIUS, 0, Math.PI * 2);
  ctx.fillStyle = props.accent;
  ctx.shadowColor = props.accent;
  ctx.shadowBlur = DOT_SHADOW_BASE + DOT_SHADOW_PEAK * flashStrength;
  ctx.fill();
  ctx.shadowBlur = 0;

  rafId = requestAnimationFrame(draw);
}

onMounted(() => {
  rafId = requestAnimationFrame(draw);
});

onUnmounted(() => {
  cancelAnimationFrame(rafId);
});
</script>

<style scoped>
.phase-ring {
  display: block;
  width: 100%;
  aspect-ratio: 1;
}
</style>
