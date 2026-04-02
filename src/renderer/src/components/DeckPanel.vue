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
      <span v-if="deck.trackName" class="deck__track-name" :title="deck.trackName">{{ deck.trackName }}</span>
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

    <div v-if="deck.detecting" class="deck__detecting">
      <span class="deck__detecting-text">Detecting BPM...</span>
    </div>

    <WaveformDisplay v-show="deck.mode === 'edit' && !deck.detecting" :deck-id="deckId" class="deck__waveform" />

    <template v-if="deck.mode === 'play' && !deck.detecting">
      <div v-if="!deck.trackLoaded" class="deck__no-track">NO TRACK LOADED</div>

      <div class="phase-ring-wrapper">
        <PhaseRing :deck-id="deckId" />
      </div>

      <div class="deck__bpm-display">
        <input
          v-if="editingBpm"
          ref="bpmInputEl"
          class="deck__bpm-input"
          type="number"
          min="20"
          step="0.1"
          :value="deck.targetBpm.toFixed(1)"
          @blur="onBpmInputBlur"
          @keydown.enter="onBpmInputBlur"
          @keydown.escape="editingBpm = false"
        />
        <span v-else class="deck__bpm-value" @click="startEditingBpm">{{ deck.targetBpm.toFixed(1) }}</span>
        <span class="deck__bpm-unit">BPM</span>
        <span class="deck__bpm-inferred" v-if="deck.loopRegion">({{ deck.inferredBpm.toFixed(1) }})</span>
      </div>

      <div class="deck__slider-wrapper">
        <button class="deck__bpm-step" @mousedown.prevent="startBpmStep(1)" @mouseup="stopBpmStep" @mouseleave="stopBpmStep">▲</button>
        <span class="deck__slider-label">+{{ PITCH_RANGE }}%</span>
        <input
          type="range"
          class="deck__slider"
          :min="-PITCH_RANGE"
          :max="PITCH_RANGE"
          step="0.1"
          :value="deck.pitchOffset"
          orient="vertical"
          @input="onSliderInput"
          @dblclick="deck.setPitchOffset(0)"
        />
        <span class="deck__slider-label">-{{ PITCH_RANGE }}%</span>
        <button class="deck__bpm-step" @mousedown.prevent="startBpmStep(-1)" @mouseup="stopBpmStep" @mouseleave="stopBpmStep">▼</button>
      </div>

      <div class="deck__eq-row">
        <div v-for="band in (['low', 'mid', 'high'] as const)" :key="band" class="deck__eq-band">
          <input
            type="range"
            class="deck__slider deck__slider--eq"
            min="-12"
            max="12"
            step="0.5"
            :value="deck.eq[band]"
            orient="vertical"
            @input="e => deck.setEq(band, parseFloat((e.target as HTMLInputElement).value))"
            @dblclick="deck.setEq(band, 0)"
          />
          <span class="deck__slider-label">{{ band.toUpperCase() }}</span>
        </div>
      </div>

      <div class="deck__nudge-row">
        <button
          class="deck__btn deck__btn--nudge"
          :class="{ 'deck__btn--active': deck.nudging === 'back' }"
          @mousedown="deck.nudgeStart('back')"
          @mouseup="deck.nudgeEnd()"
          @mouseleave="deck.nudgeEnd()"
        >
          <span class="deck__btn-key">{{ deckId === 'A' ? 'Q' : 'I' }}</span>
          <span class="deck__btn-icon">◀◀</span>
        </button>
        <button
          class="deck__btn deck__btn--nudge"
          :class="{ 'deck__btn--active': deck.nudging === 'forward' }"
          @mousedown="deck.nudgeStart('forward')"
          @mouseup="deck.nudgeEnd()"
          @mouseleave="deck.nudgeEnd()"
        >
          <span class="deck__btn-key">{{ deckId === 'A' ? 'W' : 'O' }}</span>
          <span class="deck__btn-icon">▶▶</span>
        </button>
      </div>

      <div class="deck__transport-row">
        <button
          class="deck__btn deck__btn--cue"
          :class="{ 'deck__btn--cueing': deck.cueing }"
          @mousedown.prevent="deck.cueStart()"
          @mouseup="deck.cueEnd()"
          @mouseleave="deck.cueEnd()"
        >
          <span class="deck__btn-key">{{ deckId === 'A' ? 'A' : 'K' }}</span>
          <span>CUE</span>
        </button>
        <button
          class="deck__btn deck__btn--play"
          :class="{ 'deck__btn--playing': deck.playing }"
          @click="deck.togglePlay()"
        >
          <span class="deck__btn-key">{{ deckId === 'A' ? 'S' : 'L' }}</span>
          <span>{{ deck.playing ? '⏸' : '▶' }}</span>
        </button>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, nextTick } from 'vue'
import { useDecksStore, PITCH_RANGE } from '@renderer/stores/decks'
import type { DeckId } from '@renderer/stores/decks'
import PhaseRing from '@renderer/components/PhaseRing.vue'
import WaveformDisplay from '@renderer/components/WaveformDisplay.vue'

const props = defineProps<{ deckId: DeckId }>()

const store = useDecksStore()
const deck = computed(() => store.decks[props.deckId])

function onSliderInput(e: Event) {
  const val = parseFloat((e.target as HTMLInputElement).value)
  deck.value.setPitchOffset(val)
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
  if (!isNaN(val) && val > 0) deck.value.setTargetBpm(val)
  editingBpm.value = false
}

// Step buttons adjust targetBpm by 0.1
let stepInterval: ReturnType<typeof setInterval> | null = null

function startBpmStep(dir: 1 | -1) {
  deck.value.setTargetBpm(deck.value.targetBpm + dir * 0.1)
  stepInterval = setInterval(() => {
    deck.value.setTargetBpm(deck.value.targetBpm + dir * 0.1)
  }, 80)
}

function stopBpmStep() {
  if (stepInterval !== null) { clearInterval(stepInterval); stepInterval = null }
}
</script>
