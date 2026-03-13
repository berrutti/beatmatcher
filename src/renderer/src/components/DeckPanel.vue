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
    <!-- Header (always visible) -->
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

    <!-- EDIT MODE -->
    <WaveformDisplay v-if="deck.mode === 'edit'" :deck-id="deckId" class="deck__waveform" />

    <!-- PLAY MODE -->
    <template v-else>
      <PhaseRing :deck-id="deckId" />

      <div class="deck__bpm-display">
        <span class="deck__bpm-value">{{ deck.displayBpm.toFixed(1) }}</span>
        <span class="deck__bpm-unit">BPM</span>
      </div>

      <!-- BPM Slider — only shown when no loop is loaded (manual BPM mode) -->
      <div class="deck__slider-wrapper" v-if="!deck.trackLoaded">
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
      </div>

      <!-- Pulse toggle -->
      <button
        class="deck__pulse-btn"
        :class="{ 'deck__pulse-btn--on': deck.pulseEnabled }"
        @click="deck.togglePulse()"
      >
        <span class="deck__btn-key">{{ deckId === 'A' ? 'TAB' : '\'' }}</span>
        <span>PULSE</span>
      </button>

      <!-- Nudge buttons -->
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
          <span class="deck__btn-icon">▶▶</span>
          <span class="deck__btn-key">{{ deckId === 'A' ? 'W' : 'P' }}</span>
        </button>
      </div>

      <!-- Transport -->
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
import { computed } from 'vue'
import { useDecksStore } from '@renderer/stores/decks'
import type { DeckId } from '@renderer/stores/decks'
import PhaseRing from '@renderer/components/PhaseRing.vue'
import WaveformDisplay from '@renderer/components/WaveformDisplay.vue'

const props = defineProps<{ deckId: DeckId }>()

const store = useDecksStore()
const deck = computed(() => store.decks[props.deckId])

const BPM_MIN = 60
const BPM_MAX = 200

function onSliderInput(e: Event) {
  const val = parseFloat((e.target as HTMLInputElement).value)
  deck.value.setBpm(val)
}
</script>
