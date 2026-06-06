<template>
  <div class="timeline" ref="containerEl">
    <canvas ref="canvasEl" class="timeline__canvas" @click="onCanvasClick" />
  </div>
</template>

<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from 'vue';
import type { Clip, LoadedSpan } from '@renderer/composables/useSessionTimeline';
import { formatMs } from '@renderer/utils/time';

const DECK_ORDER = ['A', 'B', 'C', 'D'] as const;
const DECK_ACCENT: Record<string, string> = {
  A: '#3b82f6',
  B: '#f97316',
  C: '#208043',
  D: '#d631b0'
};
const ROW_H = 36;
const LABEL_W = 32;
const TICK_H = 16;
const PADDING = 8;

const props = defineProps<{
  durationMs: number;
  clips: Clip[];
  loadedSpans: LoadedSpan[];
  playheadMs: number;
}>();

const emit = defineEmits<{ seek: [ms: number] }>();

function onCanvasClick(e: MouseEvent) {
  if (!props.durationMs || !canvasEl.value) return;
  const rect = canvasEl.value.getBoundingClientRect();
  const x = e.clientX - rect.left;
  const trackW = rect.width - LABEL_W;
  if (trackW <= 0) return;
  const frac = Math.max(0, Math.min(1, (x - LABEL_W) / trackW));
  emit('seek', frac * props.durationMs);
}

const containerEl = ref<HTMLDivElement | null>(null);
const canvasEl = ref<HTMLCanvasElement | null>(null);
let ro: ResizeObserver | null = null;

function draw() {
  const canvas = canvasEl.value;
  const container = containerEl.value;
  if (!canvas || !container) return;

  const dpr = window.devicePixelRatio || 1;
  const w = container.clientWidth;
  const h = container.clientHeight;
  if (w === 0 || h === 0) return;

  canvas.width = w * dpr;
  canvas.height = h * dpr;
  canvas.style.width = w + 'px';
  canvas.style.height = h + 'px';

  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.scale(dpr, dpr);
  ctx.clearRect(0, 0, w, h);

  const trackW = w - LABEL_W - PADDING;
  const totalMs = props.durationMs || 1;

  function msToX(ms: number) {
    return LABEL_W + (ms / totalMs) * trackW;
  }

  // Background
  ctx.fillStyle = 'var(--color-bg, #111)';
  ctx.fillRect(0, 0, w, h);

  // Tick marks + time labels
  ctx.fillStyle = '#444';
  ctx.fillRect(LABEL_W, 0, trackW, 1);

  const tickIntervalMs = chooseTickInterval(totalMs, trackW);
  ctx.font = `9px monospace`;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'alphabetic';
  for (let ms = 0; ms <= totalMs; ms += tickIntervalMs) {
    const x = msToX(ms);
    ctx.fillStyle = '#333';
    ctx.fillRect(x, 0, 1, TICK_H);
    ctx.fillStyle = '#555';
    ctx.fillText(formatMs(ms), x + 3, TICK_H - 3);
  }

  // Deck rows
  for (let ri = 0; ri < DECK_ORDER.length; ri++) {
    const deckId = DECK_ORDER[ri];
    const y = TICK_H + ri * ROW_H;
    const accent = DECK_ACCENT[deckId];

    // Row background
    ctx.fillStyle = ri % 2 === 0 ? '#161616' : '#131313';
    ctx.fillRect(0, y, w, ROW_H);

    // Deck label
    ctx.font = `bold 9px monospace`;
    ctx.fillStyle = accent;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(deckId, LABEL_W / 2, y + ROW_H / 2);

    // Row divider
    ctx.fillStyle = '#222';
    ctx.fillRect(0, y + ROW_H - 1, w, 1);

    // Loaded spans (track-on-deck background)
    const deckSpans = props.loadedSpans.filter((s) => s.deck === deckId);
    for (const span of deckSpans) {
      const sx = msToX(span.startMs);
      const sw = Math.max(2, msToX(span.endMs) - sx);
      const sy = y + 4;
      const sh = ROW_H - 8;

      ctx.fillStyle = accent + '18';
      ctx.fillRect(sx, sy, sw, sh);

      ctx.strokeStyle = accent + '40';
      ctx.lineWidth = 1;
      ctx.strokeRect(sx + 0.5, sy + 0.5, sw - 1, sh - 1);

      if (sw > 40) {
        ctx.font = `9px monospace`;
        ctx.fillStyle = accent + '55';
        ctx.textAlign = 'left';
        ctx.textBaseline = 'middle';
        ctx.save();
        ctx.rect(sx + 3, sy, sw - 6, sh);
        ctx.clip();
        ctx.fillText(span.trackName, sx + 3, sy + sh / 2);
        ctx.restore();
      }
    }

    // Clips (actual playback rectangles)
    const deckClips = props.clips.filter((c) => c.deck === deckId);
    for (const clip of deckClips) {
      const cx = msToX(clip.sessionStartMs);
      const cw = Math.max(2, msToX(clip.sessionEndMs) - cx);
      const cy = y + 4;
      const ch = ROW_H - 8;

      ctx.fillStyle = accent + '55';
      ctx.fillRect(cx, cy, cw, ch);

      ctx.strokeStyle = accent + 'cc';
      ctx.lineWidth = 1;
      ctx.strokeRect(cx + 0.5, cy + 0.5, cw - 1, ch - 1);
    }
  }

  // Playhead
  if (props.playheadMs > 0) {
    const px = msToX(props.playheadMs);
    ctx.strokeStyle = '#ffffff88';
    ctx.lineWidth = 1;
    ctx.setLineDash([3, 3]);
    ctx.beginPath();
    ctx.moveTo(px, 0);
    ctx.lineTo(px, h);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  // Separator between labels and track area
  ctx.fillStyle = '#2a2a2a';
  ctx.fillRect(LABEL_W - 1, 0, 1, h);
}

function chooseTickInterval(totalMs: number, availPx: number): number {
  const candidates = [1000, 2000, 5000, 10000, 15000, 30000, 60000, 120000, 300000, 600000];
  const minGapPx = 60;
  for (const ms of candidates) {
    if ((ms / totalMs) * availPx >= minGapPx) return ms;
  }
  return candidates[candidates.length - 1];
}

onMounted(() => {
  ro = new ResizeObserver(() => requestAnimationFrame(draw));
  if (containerEl.value) ro.observe(containerEl.value);
  requestAnimationFrame(draw);
});

onUnmounted(() => {
  ro?.disconnect();
});

watch(
  () => [props.clips, props.loadedSpans, props.durationMs, props.playheadMs],
  () => {
    requestAnimationFrame(draw);
  },
  { deep: true }
);
</script>

<style scoped>
.timeline {
  width: 100%;
  height: 100%;
  overflow: hidden;
  position: relative;
}

.timeline__canvas {
  display: block;
}
</style>
