<template>
  <div
    class="deck"
    :style="{ '--deck-accent': props.deck.accent }"
    :class="{
      'deck--playing': props.deck.playing,
      'deck--edit': props.deck.mode === 'edit',
      'deck--drag-over': isDragOver && props.deck.mode === 'play'
    }"
    @dragover="onDeckDragOver"
    @dragleave="onDeckDragLeave"
    @drop="onDeckDrop"
  >
    <ConfirmModal
      :open="pendingFile !== null"
      title="Load new track?"
      body="Playback will stop and the current track will be replaced."
      @confirm="onConfirmLoad"
      @cancel="pendingFile = null"
    />

    <BpmModal
      :open="bpmModalOpen"
      :current-bpm="props.deck.trackBpm > 0 ? props.deck.trackBpm : null"
      @submit="onBpmModalSubmit"
      @cancel="bpmModalOpen = false"
    />

    <div class="deck__header">
      <span class="deck__label">DECK {{ props.deck.id }}</span>
      <div class="deck__status-dot" :class="{ 'deck__status-dot--on': props.deck.playing }" />
      <span v-if="props.deck.trackName" class="deck__track-name" :title="props.deck.trackName">{{
        props.deck.trackName
      }}</span>
      <div class="deck__mode-tabs">
        <button
          class="deck__mode-tab"
          :class="{ 'deck__mode-tab--active': props.deck.mode === 'edit' }"
          @click="props.deck.setEditMode()"
        >
          EDIT
        </button>
        <button
          class="deck__mode-tab"
          :class="{ 'deck__mode-tab--active': props.deck.mode === 'play' }"
          @click="props.deck.setPlayMode()"
        >
          PLAY
        </button>
      </div>
    </div>

    <div v-if="props.deck.detecting" class="deck__detecting">
      <span class="deck__detecting-text">Detecting BPM...</span>
    </div>

    <WaveformDisplay
      v-show="props.deck.mode === 'edit' && !props.deck.detecting"
      class="deck__waveform"
      :accent="props.deck.accent"
      :buffer="props.deck.buffer"
      :loop-region="props.deck.loopRegion"
      :loop-active="props.deck.loopActive"
      :track-bpm="props.deck.trackBpm"
      :beat-offset="props.deck.beatOffset"
      :cue-point="props.deck.cuePoint"
      :get-track-position="() => props.deck.trackPosition"
      @load="onLoadFile"
      @set-region="props.deck.setLoopRegion"
      @move-region="props.deck.moveLoopRegion"
      @set-beat-offset="props.deck.setBeatOffset"
      @request-bpm-input="bpmModalOpen = true"
    />

    <div v-show="props.deck.mode === 'play' && !props.deck.detecting" class="phase-ring-wrapper">
      <PhaseRing
        :accent="props.deck.accent"
        :track-bpm="props.deck.trackBpm"
        :beat-offset="props.deck.beatOffset"
        :get-track-position="() => props.deck.trackPosition"
      />
    </div>

    <template v-if="props.deck.mode === 'play' && !props.deck.detecting">
      <div v-if="!props.deck.trackLoaded" class="deck__drop-zone">
        <button class="deck__load-btn" @click="openPlayFileDialog">LOAD TRACK</button>
      </div>

      <div class="deck__bpm-display" v-if="props.deck.trackLoaded">
        <input
          v-if="editingBpm"
          ref="bpmInputEl"
          class="deck__bpm-input"
          type="number"
          min="20"
          step="0.1"
          :value="props.deck.targetBpm?.toFixed(1) ?? ''"
          @blur="onBpmInputBlur"
          @keydown.enter="onBpmInputBlur"
          @keydown.escape="editingBpm = false"
        />
        <span v-else class="deck__bpm-value" @click="onBpmValueClick">{{
          props.deck.targetBpm?.toFixed(1) ?? '--.-'
        }}</span>
        <span class="deck__bpm-unit">BPM</span>
        <span
          class="deck__bpm-inferred"
          v-if="props.deck.loopRegion && props.deck.trackBpm !== null"
          >({{ props.deck.trackBpm.toFixed(1) }})</span
        >
      </div>

      <div class="deck__slider-wrapper">
        <button
          class="deck__bpm-step"
          :disabled="!props.deck.trackLoaded"
          @mousedown.prevent="onBpmStepMouseDown(1)"
          @mouseup="stopBpmStep"
          @mouseleave="stopBpmStep"
        >
          ▲
        </button>
        <span class="deck__slider-label">+{{ PITCH_RANGE }}%</span>
        <input
          type="range"
          class="deck__slider"
          :min="-PITCH_RANGE"
          :max="PITCH_RANGE"
          step="0.1"
          :value="props.deck.pitchOffset"
          orient="vertical"
          :disabled="!props.deck.trackLoaded"
          @input="onSliderInput"
          @dblclick="onPitchDblClick"
        />
        <span class="deck__slider-label">-{{ PITCH_RANGE }}%</span>
        <button
          class="deck__bpm-step"
          :disabled="!props.deck.trackLoaded"
          @mousedown.prevent="onBpmStepMouseDown(-1)"
          @mouseup="stopBpmStep"
          @mouseleave="stopBpmStep"
        >
          ▼
        </button>
      </div>

      <div class="deck__eq-row">
        <div v-for="band in ['low', 'mid', 'high'] as const" :key="band" class="deck__eq-band">
          <input
            type="range"
            class="deck__slider deck__slider--eq"
            min="-12"
            max="12"
            step="0.5"
            :value="props.deck.eq[band]"
            orient="vertical"
            :disabled="!props.deck.trackLoaded"
            @input="(e) => onEqInput(band, e)"
            @dblclick="onEqDblClick(band)"
          />
          <span class="deck__slider-label">{{ band.toUpperCase() }}</span>
        </div>
      </div>

      <div class="deck__nudge-row">
        <button
          class="deck__btn deck__btn--nudge"
          :class="{ 'deck__btn--active': props.deck.nudging === 'back' }"
          :disabled="!props.deck.trackLoaded"
          :tabindex="-1"
          @mousedown="onNudgeStart('back')"
          @mouseup="props.deck.nudgeEnd()"
          @mouseleave="props.deck.nudgeEnd()"
        >
          <span class="deck__btn-key" :tabindex="-1">{{ props.keybindings.nudgeBack }}</span>
          <span class="deck__btn-icon">◀◀</span>
        </button>
        <button
          class="deck__btn deck__btn--nudge"
          :class="{ 'deck__btn--active': props.deck.nudging === 'forward' }"
          :disabled="!props.deck.trackLoaded"
          :tabindex="-1"
          @mousedown="onNudgeStart('forward')"
          @mouseup="props.deck.nudgeEnd()"
          @mouseleave="props.deck.nudgeEnd()"
        >
          <span class="deck__btn-key">{{ props.keybindings.nudgeForward }}</span>
          <span class="deck__btn-icon">▶▶</span>
        </button>
      </div>

      <div class="deck__transport-row">
        <button
          class="deck__btn deck__btn--cue"
          :class="{ 'deck__btn--cueing': props.deck.cueing }"
          :disabled="!props.deck.trackLoaded"
          :tabindex="-1"
          @mousedown.prevent="onCueMouseDown()"
          @mouseup="props.deck.cueEnd()"
          @mouseleave="props.deck.cueEnd()"
        >
          <span class="deck__btn-key">{{ props.keybindings.cue }}</span>
          <span>CUE</span>
        </button>
        <button
          class="deck__btn deck__btn--play"
          :class="{ 'deck__btn--playing': props.deck.playing }"
          :disabled="!props.deck.trackLoaded"
          :tabindex="-1"
          @click="onTogglePlay()"
        >
          <span class="deck__btn-key">{{ props.keybindings.play }}</span>
          <span>{{ props.deck.playing ? '⏸' : '▶' }}</span>
        </button>
        <button
          class="deck__btn deck__btn--loop-in"
          :disabled="!props.deck.trackLoaded"
          :tabindex="-1"
          @click="props.deck.setLoopIn()"
        >
          <span class="deck__btn-label">IN</span>
        </button>
        <button
          class="deck__btn deck__btn--loop-out"
          :class="{ 'deck__btn--loop-active': props.deck.loopActive }"
          :disabled="!props.deck.trackLoaded"
          :tabindex="-1"
          @click="props.deck.setLoopOut()"
        >
          <span class="deck__btn-label">OUT</span>
        </button>
        <button
          class="deck__btn deck__btn--reloop"
          :class="{ 'deck__btn--loop-active': props.deck.loopActive }"
          :disabled="!props.deck.loopRegion"
          :tabindex="-1"
          @click="props.deck.reloopOrExit()"
        >
          <span class="deck__btn-label">{{ props.deck.loopActive ? 'EXIT' : 'RELOOP' }}</span>
        </button>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, nextTick } from 'vue';
import { PITCH_RANGE } from '@renderer/stores/decks';
import type { Deck } from '@renderer/stores/decks';
import { useAudioFileDrop } from '@renderer/composables/useAudioFileDrop';
import PhaseRing from '@renderer/components/PhaseRing.vue';
import WaveformDisplay from '@renderer/components/WaveformDisplay.vue';
import ConfirmModal from '@renderer/components/ConfirmModal.vue';
import BpmModal from '@renderer/components/BpmModal.vue';

const BPM_STEP = 0.1;
const BPM_STEP_INTERVAL_MS = 80;

const props = defineProps<{
  deck: Deck;
  keybindings: { cue: string; play: string; nudgeBack: string; nudgeForward: string };
}>();

// BPM text editing
const editingBpm = ref(false);
const bpmInputEl = ref<HTMLInputElement | null>(null);

async function startEditingBpm() {
  editingBpm.value = true;
  await nextTick();
  bpmInputEl.value?.select();
}

function onBpmValueClick() {
  if (!props.deck.trackLoaded) return;
  startEditingBpm();
}

function onBpmInputBlur(e: Event) {
  const val = parseFloat((e.target as HTMLInputElement).value);
  if (!isNaN(val) && val > 0) props.deck.setTargetBpm(val);
  editingBpm.value = false;
}

function onSliderInput(e: Event) {
  if (!props.deck.trackLoaded) return;
  const val = parseFloat((e.target as HTMLInputElement).value);
  props.deck.setPitchOffset(val);
}

function onPitchDblClick() {
  if (!props.deck.trackLoaded) return;
  props.deck.setPitchOffset(0);
}

function onEqInput(band: 'low' | 'mid' | 'high', e: Event) {
  if (!props.deck.trackLoaded) return;
  props.deck.setEq(band, parseFloat((e.target as HTMLInputElement).value));
}

function onEqDblClick(band: 'low' | 'mid' | 'high') {
  if (!props.deck.trackLoaded) return;
  props.deck.setEq(band, 0);
}

// Step buttons adjust targetBpm by 0.1
let stepInterval: ReturnType<typeof setInterval> | null = null;

function onBpmStepMouseDown(dir: 1 | -1) {
  if (!props.deck.trackLoaded || props.deck.targetBpm === null) return;
  props.deck.setTargetBpm(props.deck.targetBpm + dir * BPM_STEP);
  stepInterval = setInterval(() => {
    if (props.deck.targetBpm === null) return;
    props.deck.setTargetBpm(props.deck.targetBpm + dir * BPM_STEP);
  }, BPM_STEP_INTERVAL_MS);
}

function stopBpmStep() {
  if (stepInterval !== null) {
    clearInterval(stepInterval);
    stepInterval = null;
  }
}

function onNudgeStart(direction: 'back' | 'forward') {
  if (!props.deck.trackLoaded) return;
  props.deck.nudgeStart(direction);
}

function onCueMouseDown() {
  if (!props.deck.trackLoaded) return;
  if (props.deck.playing) {
    props.deck.setCueAndStop();
  } else {
    props.deck.cueStart();
  }
}

function onTogglePlay() {
  if (!props.deck.trackLoaded) return;
  props.deck.togglePlay();
}

const pendingFile = ref<File | null>(null);
const bpmModalOpen = ref(false);

const {
  isDragOver,
  onDragOver: onDeckDragOver,
  onDragLeave: onDeckDragLeave,
  onDrop: onDeckDrop,
  openFileDialog: openPlayFileDialog
} = useAudioFileDrop((file) => onLoadFile(file));

function onLoadFile(file: File) {
  if (props.deck.loopPlaying) {
    pendingFile.value = file;
    return;
  }
  props.deck.loadTrack(file, () => {
    bpmModalOpen.value = true;
  });
}

function onConfirmLoad() {
  const file = pendingFile.value;
  pendingFile.value = null;
  if (file)
    props.deck.loadTrack(file, () => {
      bpmModalOpen.value = true;
    });
}

function onBpmModalSubmit(bpm: number) {
  props.deck.setTrackBpm(bpm);
  props.deck.setEditMode();
  bpmModalOpen.value = false;
}
</script>

<style scoped>
.deck {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 0.3em 1.5em;
  gap: 0.45em;
  transition: background 0.2s;
}

.deck--playing {
  background: color-mix(in srgb, var(--deck-accent) 4%, transparent);
}

.deck--edit {
  padding: 0;
  justify-content: flex-start;
}
.deck--edit .deck__header {
  padding: 0.6em 1em;
  width: 100%;
  border-bottom: 1px solid var(--color-border);
}

.deck__waveform {
  flex: 1;
  width: 100%;
  min-height: 0;
}

.deck__detecting {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}
.deck__detecting-text {
  font-size: 1em;
  opacity: 0.6;
  animation: pulse-fade 1.2s ease-in-out infinite;
}
@keyframes pulse-fade {
  0%,
  100% {
    opacity: 0.3;
  }
  50% {
    opacity: 0.8;
  }
}

.deck--drag-over {
  outline: 2px dashed var(--deck-accent);
  outline-offset: -4px;
}

.deck__drop-zone {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 0.5em 0;
}
.deck__load-btn {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.65em;
  letter-spacing: 0.15em;
  padding: 6px 16px;
  border-radius: 4px;
  cursor: pointer;
}
.deck__load-btn:hover {
  border-color: #555;
  color: var(--color-text);
}

.deck__header {
  display: flex;
  align-items: center;
  gap: 0.6em;
  width: 100%;
}

.deck__label {
  font-size: 0.9em;
  font-weight: 700;
  letter-spacing: 0.25em;
  color: var(--deck-accent);
}

.deck__status-dot {
  width: 0.5em;
  height: 0.5em;
  border-radius: 50%;
  background: var(--color-border);
  transition:
    background 0.1s,
    box-shadow 0.1s;
}
.deck__status-dot--on {
  background: var(--color-play);
  box-shadow: 0 0 0.5em var(--color-play);
}

.deck__track-name {
  font-size: 0.65em;
  color: var(--color-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 12em;
}

.deck__mode-tabs {
  display: flex;
  gap: 0.3em;
  margin-left: auto;
}
.deck__mode-tab {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.65em;
  letter-spacing: 0.15em;
  padding: 0.25em 0.7em;
  border-radius: 3px;
  cursor: pointer;
}
.deck__mode-tab--active {
  border-color: currentColor;
  color: var(--deck-accent);
}

.phase-ring-wrapper {
  width: 8em;
  flex-shrink: 0;
}

.deck__bpm-display {
  display: flex;
  align-items: baseline;
  gap: 0.3em;
}

.deck__bpm-value {
  font-size: 3em;
  font-weight: 700;
  letter-spacing: -0.02em;
  line-height: 1;
  font-variant-numeric: tabular-nums;
  cursor: text;
  color: var(--deck-accent);
}

.deck__bpm-input {
  font-size: 3em;
  font-weight: 700;
  font-family: var(--font);
  letter-spacing: -0.02em;
  line-height: 1;
  font-variant-numeric: tabular-nums;
  background: transparent;
  border: none;
  border-bottom: 2px solid currentColor;
  color: var(--deck-accent);
  width: 5ch;
  padding: 0;
  outline: none;
}

.deck__bpm-unit {
  font-size: 0.9em;
  color: var(--color-muted);
  letter-spacing: 0.1em;
}
.deck__bpm-inferred {
  font-size: 0.7em;
  color: var(--color-muted);
  opacity: 0.6;
}

.deck__slider-wrapper {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.25em;
  width: 3em;
}

.deck__slider-label {
  font-size: 0.65em;
  color: var(--color-muted);
}

.deck__bpm-step {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-size: 0.65em;
  width: 2.2em;
  height: 1.4em;
  border-radius: 3px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  user-select: none;
}
.deck__bpm-step:hover {
  border-color: #555;
  color: var(--color-text);
}

.deck__slider {
  -webkit-appearance: slider-vertical;
  appearance: auto;
  writing-mode: vertical-lr;
  direction: rtl;
  width: 28px;
  height: 7em;
  cursor: pointer;
  accent-color: var(--deck-accent);
}

.deck__nudge-row,
.deck__transport-row {
  display: flex;
  gap: 0.5em;
}

.deck__btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.25em;
  width: 5.5em;
  height: 4.5em;
  border: 1px solid var(--color-border);
  border-radius: var(--radius);
  background: var(--color-surface);
  color: var(--color-text);
  font-family: var(--font);
  font-size: 1em;
  cursor: pointer;
  transition:
    background 0.1s,
    border-color 0.1s,
    box-shadow 0.1s;
}
.deck__btn:hover {
  border-color: #444;
  background: #1e1e1e;
}

.deck__btn-key {
  font-size: 0.6em;
  color: var(--color-muted);
  letter-spacing: 0.15em;
}
.deck__btn-icon {
  font-size: 0.9em;
}
.deck__btn-label {
  font-size: 0.6em;
  letter-spacing: 0.15em;
}

.deck__btn--nudge.deck__btn--active {
  background: color-mix(in srgb, var(--color-nudge) 20%, transparent);
  border-color: var(--color-nudge);
  box-shadow: 0 0 0.8em color-mix(in srgb, var(--color-nudge) 30%, transparent);
  color: var(--color-nudge);
}
.deck__btn--cue:hover {
  border-color: var(--color-cue);
  color: var(--color-cue);
}
.deck__btn--cueing {
  border-color: var(--color-cue) !important;
  color: var(--color-cue) !important;
  background: color-mix(in srgb, var(--color-cue) 12%, transparent) !important;
}
.deck__btn--play:hover {
  border-color: var(--deck-accent);
  color: var(--deck-accent);
}
.deck__btn--play.deck__btn--playing {
  border-color: var(--color-play);
  color: var(--color-play);
  background: color-mix(in srgb, var(--color-play) 8%, transparent);
}
.deck__btn--loop-in:hover:not(:disabled),
.deck__btn--loop-out:hover:not(:disabled) {
  border-color: var(--deck-accent);
  color: var(--deck-accent);
}
.deck__btn--loop-out.deck__btn--loop-active {
  border-color: var(--deck-accent);
  color: var(--deck-accent);
  background: color-mix(in srgb, var(--deck-accent) 15%, transparent);
}
.deck__btn--reloop:hover:not(:disabled) {
  border-color: var(--deck-accent);
  color: var(--deck-accent);
}
.deck__btn--reloop.deck__btn--loop-active {
  border-color: var(--deck-accent);
  color: var(--deck-accent);
  background: color-mix(in srgb, var(--deck-accent) 15%, transparent);
  box-shadow: 0 0 0.8em color-mix(in srgb, var(--deck-accent) 25%, transparent);
}

.deck__eq-row {
  display: flex;
  gap: 1.2em;
}
.deck__eq-band {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.25em;
}
.deck__slider--eq {
  height: 4em;
  width: 22px;
}
</style>
