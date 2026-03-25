<template>
  <div
    class="deck"
    :class="{
      'deck--playing': deck.playing,
      'deck--a': deckId === 'A',
      'deck--b': deckId === 'B',
      'deck--edit': deck.mode === 'edit'
    }"
  >
    <div class="deck__header">
      <span class="deck__label">DECK {{ deckId }}</span>
      <div class="deck__status-dot" :class="{ 'deck__status-dot--on': deck.playing }" />
      <div class="deck__mode-tabs">
        <button
          class="deck__mode-tab"
          :class="{ 'deck__mode-tab--active': deck.mode === 'edit' }"
          @click="deck.mode = 'edit'"
        >EDIT</button>
        <button
          class="deck__mode-tab"
          :class="{ 'deck__mode-tab--active': deck.mode === 'play' }"
          @click="deck.mode = 'play'"
        >PLAY</button>
      </div>
    </div>

    <WaveformDisplay v-show="deck.mode === 'edit'" :deck-id="deckId" class="deck__waveform" />

    <template v-if="deck.mode === 'play'">
      <PhaseRing :deck-id="deckId" />

      <div class="deck__bpm-display">
        <input
          v-if="editingBpm"
          ref="bpmInputEl"
          class="deck__bpm-input"
          type="number"
          :min="BPM_MIN"
          :max="BPM_MAX"
          step="0.1"
          :value="deck.displayBpm.toFixed(1)"
          @blur="onBpmInputBlur"
          @keydown.enter="onBpmInputBlur"
          @keydown.escape="editingBpm = false"
        />
        <span v-else class="deck__bpm-value" @click="startEditingBpm">{{ deck.displayBpm.toFixed(1) }}</span>
        <span class="deck__bpm-unit">BPM</span>
      </div>

      <div class="deck__slider-wrapper">
        <button class="deck__bpm-step" @mousedown.prevent="startBpmStep(1)" @mouseup="stopBpmStep" @mouseleave="stopBpmStep">▲</button>
        <span class="deck__slider-label">{{ BPM_MAX }}</span>
        <input
          type="range"
          class="deck__slider"
          :min="BPM_MIN"
          :max="BPM_MAX"
          step="0.1"
          :value="deck.bpm"
          orient="vertical"
          @input="onSliderInput"
        />
        <span class="deck__slider-label">{{ BPM_MIN }}</span>
        <button class="deck__bpm-step" @mousedown.prevent="startBpmStep(-1)" @mouseup="stopBpmStep" @mouseleave="stopBpmStep">▼</button>
      </div>

      <button
        class="deck__pulse-btn"
        :class="{ 'deck__pulse-btn--on': deck.pulseEnabled }"
        @click="deck.togglePulse()"
      >
        <span class="deck__btn-key">{{ deckId === 'A' ? 'TAB' : '\'' }}</span>
        <span>PULSE</span>
      </button>

      <div class="deck__nudge-row">
        <button
          class="deck__btn deck__btn--nudge"
          :class="{ 'deck__btn--active': deck.nudging === 'back' }"
          @mousedown="deck.nudgeStart('back')"
          @mouseup="deck.nudgeEnd()"
          @mouseleave="deck.nudgeEnd()"
        >
          <span class="deck__btn-key">{{ deckId === 'A' ? 'Q' : 'O' }}</span>
          <span class="deck__btn-icon">◀◀</span>
        </button>
        <button
          class="deck__btn deck__btn--nudge"
          :class="{ 'deck__btn--active': deck.nudging === 'forward' }"
          @mousedown="deck.nudgeStart('forward')"
          @mouseup="deck.nudgeEnd()"
          @mouseleave="deck.nudgeEnd()"
        >
          <span class="deck__btn-key">{{ deckId === 'A' ? 'W' : 'P' }}</span>
          <span class="deck__btn-icon">▶▶</span>
        </button>
      </div>

      <div class="deck__transport-row">
        <button class="deck__btn deck__btn--cue" @click="deck.cue()">
          <span class="deck__btn-key">{{ deckId === 'A' ? 'S' : 'L' }}</span>
          <span>CUE</span>
        </button>
        <button
          class="deck__btn deck__btn--play"
          :class="{ 'deck__btn--playing': deck.playing }"
          @click="deck.togglePlay()"
        >
          <span class="deck__btn-key">{{ deckId === 'A' ? 'A' : 'K' }}</span>
          <span>{{ deck.playing ? '⏸' : '▶' }}</span>
        </button>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, nextTick } from 'vue'
import { useDecksStore } from '@renderer/stores/decks'
import type { DeckId } from '@renderer/stores/decks'
import PhaseRing from '@renderer/components/PhaseRing.vue'
import WaveformDisplay from '@renderer/components/WaveformDisplay.vue'

const props = defineProps<{ deckId: DeckId }>()

const store = useDecksStore()
const deck = computed(() => store.decks[props.deckId])

const BPM_MIN = 60
const BPM_MAX = 200
const SNAP_THRESHOLD = 0.1 // snap to integer if within 10% of a beat interval

function snapBpm(val: number): number {
  const rounded = Math.round(val)
  const beatInterval = 60 / val // seconds per beat
  const snapWindow = beatInterval * SNAP_THRESHOLD
  // Convert back: if raw value is within snapWindow seconds of the rounded bpm's beat interval
  if (Math.abs(val - rounded) < SNAP_THRESHOLD) return rounded
  return Math.round(val * 10) / 10
}

function onSliderInput(e: Event) {
  const val = parseFloat((e.target as HTMLInputElement).value)
  deck.value.setBpm(snapBpm(val))
}

// BPM text editing
const editingBpm = ref(false)
const bpmInputEl = ref<HTMLInputElement | null>(null)

async function startEditingBpm() {
  editingBpm.value = true
  await nextTick()
  bpmInputEl.value?.select()
}

function onBpmInputBlur(e: Event) {
  const val = parseFloat((e.target as HTMLInputElement).value)
  if (!isNaN(val)) deck.value.setBpm(Math.max(BPM_MIN, Math.min(BPM_MAX, val)))
  editingBpm.value = false
}

// BPM step buttons (hold to repeat)
let stepInterval: ReturnType<typeof setInterval> | null = null

function startBpmStep(dir: 1 | -1) {
  deck.value.setBpm(snapBpm(deck.value.bpm + dir * 0.1))
  stepInterval = setInterval(() => {
    deck.value.setBpm(snapBpm(deck.value.bpm + dir * 0.1))
  }, 80)
}

function stopBpmStep() {
  if (stepInterval !== null) { clearInterval(stepInterval); stepInterval = null }
}
</script>
