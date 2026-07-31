<template>
  <div class="phase-ring">
    <img v-if="coverArt" :src="coverArt" class="phase-ring__art" aria-hidden="true" />
    <canvas ref="canvasEl" class="phase-ring__canvas" />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';

const props = defineProps<{
  accent: string;
  active: boolean;
  playing: boolean;
  cueing: boolean;
  getBeat: () => number | null;
  coverArt: string | null;
}>();

const LINE_WIDTH_RATIO = 0.065;

const canvasEl = ref<HTMLCanvasElement | null>(null);
let rafId = 0;
let phase4 = 0;

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
  const radius = SIZE / 2 - LINE_WIDTH / 2;

  canvas.width = SIZE * dpr;
  canvas.height = SIZE * dpr;
  ctx.scale(dpr, dpr);
  ctx.clearRect(0, 0, SIZE, SIZE);

  ctx.beginPath();
  ctx.arc(cx, cy, radius, 0, Math.PI * 2);
  ctx.strokeStyle = '#2a2a2a';
  ctx.lineWidth = LINE_WIDTH;
  ctx.stroke();

  // Held where it stopped, because scrubbing a paused deck moves the playhead at
  // hand speed and whips the ring round. A held CUE is audible, so it advances.
  if (props.playing || props.cueing) {
    const beat = props.getBeat();
    if (beat !== null) {
      phase4 = (((beat % 4) + 4) % 4) / 4;
    }
  }

  const startAngle = -Math.PI / 2;
  const endAngle = startAngle + phase4 * Math.PI * 2;
  const color = props.active ? props.accent : '#444';

  ctx.beginPath();
  ctx.arc(cx, cy, radius, startAngle, endAngle);
  ctx.strokeStyle = color;
  ctx.lineWidth = LINE_WIDTH;
  ctx.stroke();

  const dotX = cx + radius * Math.cos(endAngle);
  const dotY = cy + radius * Math.sin(endAngle);
  ctx.beginPath();
  ctx.arc(dotX, dotY, LINE_WIDTH / 2, 0, Math.PI * 2);
  ctx.fillStyle = color;
  ctx.fill();

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
  position: relative;
  width: 100%;
  aspect-ratio: 1;
}

.phase-ring__art {
  position: absolute;
  inset: 3.25%;
  width: calc(100% - 6.5%);
  height: calc(100% - 6.5%);
  border-radius: 50%;
  object-fit: cover;
  opacity: 0.65;
}

.phase-ring__canvas {
  position: absolute;
  inset: 0;
  display: block;
  width: 100%;
  height: 100%;
}
</style>
