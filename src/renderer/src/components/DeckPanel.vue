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

    <WaveformDisplay
      v-show="deck.mode === 'edit' && !deck.detecting"
      class="deck__waveform"
      :accent="deckId === 'A' ? '#3b82f6' : '#f97316'"
      :buffer="deck.buffer"
      :loop-region="deck.loopRegion"
      :loop-beats="deck.loopBeats"
      :inferred-bpm="deck.inferredBpm"
      :get-playhead-sec="() => deck.getTrackPositionSec()"
      @load="onLoadFile"
      @set-region="deck.setLoopRegion"
      @move-region="deck.moveLoopRegion"
      @set-beats="deck.setLoopBeats"
      @request-bpm-input="store.requestBpmModal(deckId)"
    />

    <div v-show="deck.mode === 'play' && !deck.detecting" class="phase-ring-wrapper">
      <PhaseRing :deck-id="deckId" />
    </div>

    <template v-if="deck.mode === 'play' && !deck.detecting">
      <div v-if="!deck.trackLoaded" class="deck__no-track">NO TRACK LOADED</div>

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
        <span v-else class="deck__bpm-value" @click="onBpmValueClick">{{ deck.trackLoaded ? deck.targetBpm.toFixed(1) : '0.0' }}</span>
        <span class="deck__bpm-unit">BPM</span>
        <span class="deck__bpm-inferred" v-if="deck.loopRegion">({{ deck.inferredBpm.toFixed(1) }})</span>
      </div>

      <div class="deck__slider-wrapper">
        <button class="deck__bpm-step" :disabled="!deck.trackLoaded" @mousedown.prevent="onBpmStepMouseDown(1)" @mouseup="stopBpmStep" @mouseleave="stopBpmStep">▲</button>
        <span class="deck__slider-label">+{{ PITCH_RANGE }}%</span>
        <input
          type="range"
          class="deck__slider"
          :min="-PITCH_RANGE"
          :max="PITCH_RANGE"
          step="0.1"
          :value="deck.pitchOffset"
          orient="vertical"
          :disabled="!deck.trackLoaded"
          @input="onSliderInput"
          @dblclick="onPitchDblClick"
        />
        <span class="deck__slider-label">-{{ PITCH_RANGE }}%</span>
        <button class="deck__bpm-step" :disabled="!deck.trackLoaded" @mousedown.prevent="onBpmStepMouseDown(-1)" @mouseup="stopBpmStep" @mouseleave="stopBpmStep">▼</button>
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
            :disabled="!deck.trackLoaded"
            @input="e => onEqInput(band, e)"
            @dblclick="onEqDblClick(band)"
          />
          <span class="deck__slider-label">{{ band.toUpperCase() }}</span>
        </div>
      </div>

      <div class="deck__nudge-row">
        <button
          class="deck__btn deck__btn--nudge"
          :class="{ 'deck__btn--active': deck.nudging === 'back' }"
          :disabled="!deck.trackLoaded"
          @mousedown="onNudgeStart('back')"
          @mouseup="deck.nudgeEnd()"
          @mouseleave="deck.nudgeEnd()"
        >
          <span class="deck__btn-key">{{ deckId === 'A' ? 'Q' : 'I' }}</span>
          <span class="deck__btn-icon">◀◀</span>
        </button>
        <button
          class="deck__btn deck__btn--nudge"
          :class="{ 'deck__btn--active': deck.nudging === 'forward' }"
          :disabled="!deck.trackLoaded"
          @mousedown="onNudgeStart('forward')"
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
          :disabled="!deck.trackLoaded"
          @mousedown.prevent="onCueStart()"
          @mouseup="deck.cueEnd()"
          @mouseleave="deck.cueEnd()"
        >
          <span class="deck__btn-key">{{ deckId === 'A' ? 'A' : 'K' }}</span>
          <span>CUE</span>
        </button>
        <button
          class="deck__btn deck__btn--play"
          :class="{ 'deck__btn--playing': deck.playing }"
          :disabled="!deck.trackLoaded"
          @click="onTogglePlay()"
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

// BPM text editing
const editingBpm = ref(false)
const bpmInputEl = ref<HTMLInputElement | null>(null)

async function startEditingBpm() {
  editingBpm.value = true
  await nextTick()
  bpmInputEl.value?.select()
}

function onBpmValueClick() {
  if (!deck.value.trackLoaded) return
  startEditingBpm()
}

function onBpmInputBlur(e: Event) {
  const val = parseFloat((e.target as HTMLInputElement).value)
  if (!isNaN(val) && val > 0) deck.value.setTargetBpm(val)
  editingBpm.value = false
}

function onSliderInput(e: Event) {
  if (!deck.value.trackLoaded) return
  const val = parseFloat((e.target as HTMLInputElement).value)
  deck.value.setPitchOffset(val)
}

function onPitchDblClick() {
  if (!deck.value.trackLoaded) return
  deck.value.setPitchOffset(0)
}

function onEqInput(band: 'low' | 'mid' | 'high', e: Event) {
  if (!deck.value.trackLoaded) return
  deck.value.setEq(band, parseFloat((e.target as HTMLInputElement).value))
}

function onEqDblClick(band: 'low' | 'mid' | 'high') {
  if (!deck.value.trackLoaded) return
  deck.value.setEq(band, 0)
}

// Step buttons adjust targetBpm by 0.1
let stepInterval: ReturnType<typeof setInterval> | null = null

function onBpmStepMouseDown(dir: 1 | -1) {
  if (!deck.value.trackLoaded) return
  deck.value.setTargetBpm(deck.value.targetBpm + dir * 0.1)
  stepInterval = setInterval(() => {
    deck.value.setTargetBpm(deck.value.targetBpm + dir * 0.1)
  }, 80)
}

function stopBpmStep() {
  if (stepInterval !== null) { clearInterval(stepInterval); stepInterval = null }
}

function onNudgeStart(direction: 'back' | 'forward') {
  if (!deck.value.trackLoaded) return
  deck.value.nudgeStart(direction)
}

function onCueStart() {
  if (!deck.value.trackLoaded) return
  deck.value.cueStart()
}

function onTogglePlay() {
  if (!deck.value.trackLoaded) return
  deck.value.togglePlay()
}

function onLoadFile(file: File) {
  if (deck.value.loopPlaying) {
    deck.value._requestLoadConfirm(file)
    return
  }
  deck.value.loadTrack(file)
}
</script>
