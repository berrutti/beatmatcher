<template>
  <div
    class="waveform"
    :class="{ 'waveform--drag-over': isDragOver }"
    @dragover.prevent="isDragOver = true"
    @dragleave="isDragOver = false"
    @drop.prevent="onDrop"
  >
    <!-- Empty state -->
    <div v-if="!deck.trackLoaded" class="waveform__empty">
      <div class="waveform__drop-icon">⊕</div>
      <p class="waveform__drop-hint">Drop audio file here</p>
      <p class="waveform__drop-hint waveform__drop-hint--sub">or</p>
      <button class="waveform__load-btn" @click="openFileDialog">LOAD TRACK</button>
    </div>

    <!-- Waveform canvas -->
    <template v-else>
      <canvas
        ref="canvasEl"
        class="waveform__canvas"
        @mousedown="onMouseDown"
        @mousemove="onMouseMove"
        @mouseup="onMouseUp"
        @mouseleave="onMouseLeave"
      />

      <!-- Beat count selector -->
      <div class="waveform__controls">
        <span class="waveform__ctrl-label">LOOP</span>
        <button
          class="waveform__beat-btn"
          :class="{ 'waveform__beat-btn--active': deck.loopBeats === 16 }"
          @click="deck.setLoopBeats(16)"
        >16</button>
        <button
          class="waveform__beat-btn"
          :class="{ 'waveform__beat-btn--active': deck.loopBeats === 32 }"
          @click="deck.setLoopBeats(32)"
        >32</button>
        <span class="waveform__bpm-readout" v-if="deck.loopRegion">
          {{ deck.displayBpm.toFixed(1) }} BPM
        </span>
        <button class="waveform__mode-btn" @click="deck.mode = 'play'" v-if="deck.loopRegion">
          ▶ PLAY MODE
        </button>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import { useDecksStore } from '@renderer/stores/decks'
import type { DeckId } from '@renderer/stores/decks'
import type { LoopRegion } from '@renderer/audio/LoopEngine'

const props = defineProps<{ deckId: DeckId }>()

const store = useDecksStore()
const deck = computed(() => store.decks[props.deckId])

const ACCENT = props.deckId === 'A' ? '#3b82f6' : '#f97316'
const HANDLE_W = 8

const canvasEl = ref<HTMLCanvasElement | null>(null)
const isDragOver = ref(false)

// Peaks data for waveform rendering
let peaks: Float32Array | null = null

// Loop region in pixel coords (derived from seconds on resize)
const loopPxStart = ref(0)
const loopPxEnd = ref(0)

// Drag state
type DragHandle = 'start' | 'end' | 'body' | null
const dragging = ref<DragHandle>(null)
const dragOffsetPx = ref(0)  // for body drag: offset from region start

// ── File loading ──────────────────────────────────────────────

async function loadFile(file: File) {
  await deck.value.loadTrack(file)
  await nextTick()
  buildPeaks()
  drawWaveform()
}

function onDrop(e: DragEvent) {
  isDragOver.value = false
  const file = e.dataTransfer?.files[0]
  if (file) loadFile(file)
}

function openFileDialog() {
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = 'audio/*'
  input.onchange = () => {
    if (input.files?.[0]) loadFile(input.files[0])
  }
  input.click()
}

// ── Peak extraction (runs once after load) ───────────────────

function buildPeaks() {
  const buf = deck.value.getLoopEngine().buffer_
  if (!buf || !canvasEl.value) return

  const canvas = canvasEl.value
  const width = canvas.clientWidth || canvas.offsetWidth || 800
  const channel = buf.getChannelData(0)
  const blockSize = Math.floor(channel.length / width)

  peaks = new Float32Array(width)
  for (let i = 0; i < width; i++) {
    let max = 0
    const offset = i * blockSize
    for (let j = 0; j < blockSize; j++) {
      const abs = Math.abs(channel[offset + j] || 0)
      if (abs > max) max = abs
    }
    peaks[i] = max
  }
}

// ── Canvas drawing ───────────────────────────────────────────

function drawWaveform() {
  const canvas = canvasEl.value
  if (!canvas || !peaks) return

  const ctx = canvas.getContext('2d')!
  const dpr = window.devicePixelRatio || 1
  const w = canvas.clientWidth
  const h = canvas.clientHeight

  // Resize canvas to match display size
  if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
    canvas.width = w * dpr
    canvas.height = h * dpr
    ctx.scale(dpr, dpr)
    buildPeaks() // rebuild at new width
  }

  ctx.clearRect(0, 0, w, h)

  // Background
  ctx.fillStyle = '#0a0a0a'
  ctx.fillRect(0, 0, w, h)

  const mid = h / 2

  // Waveform
  for (let x = 0; x < w; x++) {
    const amp = (peaks[x] || 0) * mid * 0.9
    ctx.fillStyle = '#333'
    ctx.fillRect(x, mid - amp, 1, amp * 2)
  }

  // Loop region overlay
  const region = deck.value.loopRegion
  if (region) {
    const buf = deck.value.getLoopEngine().buffer_!
    const totalDur = buf.duration
    const sx = (region.startSec / totalDur) * w
    const ex = (region.endSec / totalDur) * w
    loopPxStart.value = sx
    loopPxEnd.value = ex

    // Tinted overlay
    ctx.fillStyle = `${ACCENT}22`
    ctx.fillRect(sx, 0, ex - sx, h)

    // Waveform inside loop — brighter
    for (let x = Math.floor(sx); x < Math.ceil(ex); x++) {
      const amp = (peaks[x] || 0) * mid * 0.9
      ctx.fillStyle = ACCENT
      ctx.globalAlpha = 0.7
      ctx.fillRect(x, mid - amp, 1, amp * 2)
      ctx.globalAlpha = 1
    }

    // Handles
    drawHandle(ctx, sx, h, 'start')
    drawHandle(ctx, ex, h, 'end')

    // BPM label inside region
    const bpm = deck.value.displayBpm.toFixed(1)
    ctx.fillStyle = '#ffffff99'
    ctx.font = `bold 11px monospace`
    ctx.fillText(`${deck.value.loopBeats} beats · ${bpm} BPM`, sx + 10, 18)
  }
}

function drawHandle(ctx: CanvasRenderingContext2D, x: number, h: number, _side: 'start' | 'end') {
  ctx.fillStyle = ACCENT
  ctx.fillRect(x - HANDLE_W / 2, 0, HANDLE_W, h)
  // Triangle grip
  ctx.fillStyle = '#fff'
  const mid = h / 2
  ctx.beginPath()
  ctx.arc(x, mid, 6, 0, Math.PI * 2)
  ctx.fill()
}

// ── Drag interaction ─────────────────────────────────────────

function pxToSec(px: number): number {
  const canvas = canvasEl.value!
  const buf = deck.value.getLoopEngine().buffer_!
  return (px / canvas.clientWidth) * buf.duration
}

function hitTest(px: number): DragHandle {
  const region = deck.value.loopRegion
  if (!region) return null
  const sx = loopPxStart.value
  const ex = loopPxEnd.value
  if (Math.abs(px - sx) < HANDLE_W + 4) return 'start'
  if (Math.abs(px - ex) < HANDLE_W + 4) return 'end'
  if (px > sx && px < ex) return 'body'
  return null
}

function onMouseDown(e: MouseEvent) {
  const rect = (e.target as HTMLCanvasElement).getBoundingClientRect()
  const px = e.clientX - rect.left
  const hit = hitTest(px)

  if (hit) {
    dragging.value = hit
    dragOffsetPx.value = px - loopPxStart.value
  } else {
    // Start a new region
    dragging.value = 'end'
    const sec = pxToSec(px)
    const newRegion: LoopRegion = {
      startSec: sec,
      endSec: sec + 0.1,
      beats: deck.value.loopBeats
    }
    deck.value.setLoopRegion(newRegion)
    loopPxStart.value = px
    loopPxEnd.value = px + 1
  }
}

function onMouseMove(e: MouseEvent) {
  if (!dragging.value) {
    // Update cursor
    const rect = (e.target as HTMLCanvasElement).getBoundingClientRect()
    const px = e.clientX - rect.left
    const hit = hitTest(px)
    ;(e.target as HTMLElement).style.cursor =
      hit === 'start' || hit === 'end' ? 'ew-resize'
      : hit === 'body' ? 'grab'
      : 'crosshair'
    return
  }

  const rect = (e.target as HTMLCanvasElement).getBoundingClientRect()
  const px = e.clientX - rect.left
  const region = deck.value.loopRegion!
  const buf = deck.value.getLoopEngine().buffer_!
  const totalDur = buf.duration

  let newRegion: LoopRegion = { ...region }

  if (dragging.value === 'start') {
    const sec = Math.max(0, Math.min(pxToSec(px), region.endSec - 0.1))
    newRegion.startSec = sec
  } else if (dragging.value === 'end') {
    const sec = Math.min(totalDur, Math.max(pxToSec(px), region.startSec + 0.1))
    newRegion.endSec = sec
  } else if (dragging.value === 'body') {
    const startPx = px - dragOffsetPx.value
    const dur = region.endSec - region.startSec
    const newStart = Math.max(0, Math.min(pxToSec(startPx), totalDur - dur))
    newRegion.startSec = newStart
    newRegion.endSec = newStart + dur
  }

  deck.value.setLoopRegion(newRegion)
  drawWaveform()
}

function onMouseUp() {
  dragging.value = null
}

function onMouseLeave() {
  dragging.value = null
}

// ── Resize handling ──────────────────────────────────────────

let resizeObserver: ResizeObserver | null = null

onMounted(() => {
  if (!canvasEl.value) return
  resizeObserver = new ResizeObserver(() => {
    buildPeaks()
    drawWaveform()
  })
  resizeObserver.observe(canvasEl.value.parentElement!)
})

onUnmounted(() => {
  resizeObserver?.disconnect()
})

// Redraw when region changes externally (e.g. beat count change)
watch(() => deck.value.loopRegion, () => drawWaveform())
watch(() => deck.value.trackLoaded, async (loaded) => {
  if (loaded) {
    await nextTick()
    buildPeaks()
    drawWaveform()
  }
})
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

/* Empty state */
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

/* Canvas */
.waveform__canvas {
  flex: 1;
  width: 100%;
  display: block;
  cursor: crosshair;
}

/* Controls bar */
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
  border-color: v-bind(ACCENT);
  color: v-bind(ACCENT);
}

.waveform__bpm-readout {
  font-size: 0.85rem;
  font-weight: 700;
  color: v-bind(ACCENT);
  margin-left: auto;
  letter-spacing: 0.05em;
}

.waveform__mode-btn {
  background: transparent;
  border: 1px solid #333;
  color: #aaa;
  font-family: var(--font-mono);
  font-size: 0.65rem;
  letter-spacing: 0.15em;
  padding: 4px 12px;
  border-radius: 3px;
  cursor: pointer;
}

.waveform__mode-btn:hover {
  border-color: #666;
  color: #eee;
}
</style>
