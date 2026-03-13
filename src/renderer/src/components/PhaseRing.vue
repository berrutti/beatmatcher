<template>
  <canvas ref="canvasEl" class="phase-ring" />
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue'
import { useDecksStore } from '@renderer/stores/decks'
import { usePhase } from '@renderer/composables/usePhase'
import type { DeckId } from '@renderer/stores/decks'

const props = defineProps<{ deckId: DeckId }>()

const store = useDecksStore()
const deck = computed(() => store.decks[props.deckId])
const { getPhase } = usePhase(deck.value.getPulseEngine())

const ACCENT = props.deckId === 'A' ? '#3b82f6' : '#f97316'
const SIZE = 160
const LINE_WIDTH = 10

const canvasEl = ref<HTMLCanvasElement | null>(null)
let rafId = 0
let prevPhase = 0
let flashStrength = 0

function draw() {
  const canvas = canvasEl.value
  if (!canvas) return
  const ctx = canvas.getContext('2d')!
  const dpr = window.devicePixelRatio || 1
  const cx = SIZE / 2
  const cy = SIZE / 2
  const radius = SIZE / 2 - LINE_WIDTH

  ctx.clearRect(0, 0, SIZE * dpr, SIZE * dpr)

  // Background track
  ctx.beginPath()
  ctx.arc(cx, cy, radius, 0, Math.PI * 2)
  ctx.strokeStyle = '#2a2a2a'
  ctx.lineWidth = LINE_WIDTH
  ctx.stroke()

  const phase = getPhase()

  // Beat crossing detection
  if (deck.value.playing && prevPhase > 0.85 && phase < 0.15) {
    flashStrength = 1.0
  }
  prevPhase = phase

  // Decay flash each frame
  if (flashStrength > 0.01) {
    flashStrength *= 0.82
  } else {
    flashStrength = 0
  }

  if (phase === 0 && !deck.value.playing) {
    rafId = requestAnimationFrame(draw)
    return
  }

  const startAngle = -Math.PI / 2
  const endAngle = startAngle + phase * Math.PI * 2

  // Glow pass when flashing
  if (flashStrength > 0.01) {
    ctx.save()
    ctx.beginPath()
    ctx.arc(cx, cy, radius, startAngle, endAngle)
    ctx.strokeStyle = ACCENT
    ctx.lineWidth = LINE_WIDTH
    ctx.shadowColor = ACCENT
    ctx.shadowBlur = 24 * flashStrength
    ctx.globalAlpha = 0.4 + 0.6 * flashStrength
    ctx.stroke()
    ctx.restore()
  }

  // Main arc
  ctx.beginPath()
  ctx.arc(cx, cy, radius, startAngle, endAngle)
  ctx.strokeStyle = ACCENT
  ctx.lineWidth = LINE_WIDTH
  ctx.shadowColor = 'transparent'
  ctx.shadowBlur = 0
  ctx.globalAlpha = 1
  ctx.stroke()

  // Dot at current position
  const dotX = cx + radius * Math.cos(endAngle)
  const dotY = cy + radius * Math.sin(endAngle)
  ctx.beginPath()
  ctx.arc(dotX, dotY, LINE_WIDTH / 2 + 2, 0, Math.PI * 2)
  ctx.fillStyle = ACCENT
  ctx.shadowColor = ACCENT
  ctx.shadowBlur = 10 + 20 * flashStrength
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
  rafId = requestAnimationFrame(draw)
})

onUnmounted(() => {
  cancelAnimationFrame(rafId)
})
</script>

<style scoped>
.phase-ring {
  display: block;
}
</style>
