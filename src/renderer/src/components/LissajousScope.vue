<template>
  <div class="lissajous-wrapper">
    <span class="lissajous-label">A</span>
    <canvas ref="canvasEl" class="lissajous" />
    <span class="lissajous-label lissajous-label--b">B</span>
    <p class="lissajous-hint">phase scope</p>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { useDecksStore } from '@renderer/stores/decks'

const SIZE = 200
const FADE_ALPHA = 0.045   // trail persistence — lower = longer trail
const DOT_RADIUS = 4

const store = useDecksStore()
function getBestPhase(deckId: 'A' | 'B'): number {
  const deck = store.decks[deckId]
  const engine = deck.getLoopEngine()
  if (engine.playing) return engine.getPhase()
  return 0
}

const getPhaseA = () => getBestPhase('A')
const getPhaseB = () => getBestPhase('B')

const canvasEl = ref<HTMLCanvasElement | null>(null)
let rafId = 0

function draw() {
  const canvas = canvasEl.value
  if (!canvas) return
  const ctx = canvas.getContext('2d')!
  const half = SIZE / 2

  // Fade trail instead of clearing
  ctx.fillStyle = `rgba(10, 10, 10, ${FADE_ALPHA})`
  ctx.fillRect(0, 0, SIZE, SIZE)

  const phaseA = getPhaseA()
  const phaseB = getPhaseB()

  // Map phase 0..1 to canvas coords with padding
  // Convert to sine so 0 and 1 both map to center → smooth Lissajous figure
  const x = half + Math.sin(phaseA * Math.PI * 2) * half * 0.82
  const y = half + Math.sin(phaseB * Math.PI * 2) * half * 0.82

  // Draw dot
  ctx.beginPath()
  ctx.arc(x, y, DOT_RADIUS, 0, Math.PI * 2)
  ctx.fillStyle = '#ffffff'
  ctx.shadowColor = '#ffffff'
  ctx.shadowBlur = 12
  ctx.fill()
  ctx.shadowBlur = 0

  rafId = requestAnimationFrame(draw)
}

onMounted(() => {
  const canvas = canvasEl.value!
  const dpr = window.devicePixelRatio || 1
  canvas.width = SIZE * dpr
  canvas.height = SIZE * dpr
  canvas.style.width = `${SIZE}px`
  canvas.style.height = `${SIZE}px`
  const ctx = canvas.getContext('2d')!
  ctx.scale(dpr, dpr)

  // Init with black background so the fade works from the start
  ctx.fillStyle = '#0a0a0a'
  ctx.fillRect(0, 0, SIZE, SIZE)

  rafId = requestAnimationFrame(draw)
})

onUnmounted(() => {
  cancelAnimationFrame(rafId)
})
</script>

<style scoped>
.lissajous-wrapper {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
}

.lissajous {
  display: block;
  border-radius: 4px;
  border: 1px solid #2a2a2a;
}

.lissajous-label {
  font-size: 0.7rem;
  letter-spacing: 0.2em;
  color: #3b82f6;
  font-weight: 700;
}

.lissajous-label--b {
  color: #f97316;
}

.lissajous-hint {
  font-size: 0.6rem;
  color: #444;
  letter-spacing: 0.15em;
  margin: 0;
  text-transform: uppercase;
}
</style>
