<template>
  <div
    ref="deckEl"
    class="deck"
    :data-deck-id="props.deck.id"
    :style="{ '--deck-accent': props.deck.accent }"
    :class="{
      'deck--playing': props.deck.playing,
      'deck--drag-over': isDragOverCollection
    }"
  >
    <ConfirmModal
      :open="pendingLoad !== null"
      title="Load new track?"
      body="Playback will stop and the current track will be replaced."
      @confirm="onConfirmLoad"
      @cancel="pendingLoad = null"
    />

    <div class="deck__header">
      <span class="deck__label">DECK {{ props.deck.id }}</span>
      <div class="deck__status-dot" :class="{ 'deck__status-dot--on': props.deck.playing }" />
      <span v-if="props.deck.trackName" class="deck__track-name" :title="props.deck.trackName">{{
        props.deck.trackName
      }}</span>
      <div v-if="props.deck.trackLoaded" class="deck__bpm-header">
        <input
          v-if="editingBpm"
          ref="bpmInputEl"
          class="deck__bpm-input-header"
          type="number"
          min="20"
          step="0.1"
          :value="props.deck.targetBpm?.toFixed(1) ?? ''"
          @blur="onBpmInputBlur"
          @keydown.enter="onBpmInputBlur"
          @keydown.escape="editingBpm = false"
        />
        <span v-else class="deck__bpm-value-header" @click="onBpmValueClick">{{
          props.deck.targetBpm?.toFixed(1) ?? '--.-'
        }}</span>
        <span class="deck__bpm-unit-header">BPM</span>
      </div>
    </div>

    <div v-if="!props.deck.trackLoaded" class="deck__drop-zone">
      <span class="deck__drop-hint">Drag a track from the collection</span>
    </div>

    <template v-if="props.deck.trackLoaded">
      <OverviewWaveform
        class="deck__overview"
        :accent="props.deck.accent"
        :track-data="props.deck.trackData"
        :get-playhead-position="props.deck.getPlayheadPosition"
        :full-spectral-data="props.deck.fullSpectralData"
        @seek="props.deck.seekTo"
      />

      <div class="deck__controls">
        <div class="deck__phase-ring">
          <PhaseRing
            :accent="props.deck.accent"
            :track-bpm="props.deck.trackBpm"
            :beat-offset="props.deck.beatOffset"
            :get-track-position="() => props.deck.trackPosition"
          />
        </div>

        <div class="deck__transport-cluster">
          <div class="deck__btn-row">
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

          <div class="deck__btn-row">
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
          </div>

          <div class="deck__btn-row">
            <button
              class="deck__btn deck__btn--loop-in"
              :disabled="!props.deck.trackLoaded"
              :tabindex="-1"
              @click="props.deck.setLoopIn()"
            >
              <span class="deck__btn-icon">IN</span>
            </button>
            <button
              class="deck__btn deck__btn--loop-out"
              :class="{ 'deck__btn--loop-active': props.deck.loopActive }"
              :disabled="!props.deck.trackLoaded"
              :tabindex="-1"
              @click="props.deck.setLoopOut()"
            >
              <span class="deck__btn-icon">OUT</span>
            </button>
          </div>
        </div>

        <div class="deck__pitch-wrapper">
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
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, nextTick, watch, onMounted, onUnmounted } from 'vue';
import { PITCH_RANGE } from '@renderer/stores/decks';
import type { Deck, LoadableTrack } from '@renderer/stores/decks';
import { useCollectionStore } from '@renderer/stores/collection';
import PhaseRing from '@renderer/components/PhaseRing.vue';
import OverviewWaveform from '@renderer/components/OverviewWaveform.vue';
import ConfirmModal from '@renderer/components/ConfirmModal.vue';

const deckEl = ref<HTMLElement | null>(null);

const BPM_STEP = 0.1;
const BPM_STEP_INTERVAL_MS = 80;

const props = defineProps<{
  deck: Deck;
  keybindings: { cue: string; play: string; nudgeBack: string; nudgeForward: string };
}>();

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
    props.deck.stopAtCue();
  } else {
    props.deck.cueStart();
  }
}

function onTogglePlay() {
  if (!props.deck.trackLoaded) return;
  props.deck.togglePlay();
}

const pendingLoad = ref<LoadableTrack | null>(null);

const collectionStore = useCollectionStore();
const isDragOverCollection = ref(false);

function onWindowPointerMove(e: PointerEvent) {
  if (!deckEl.value) return;
  const rect = deckEl.value.getBoundingClientRect();
  isDragOverCollection.value =
    e.clientX >= rect.left &&
    e.clientX <= rect.right &&
    e.clientY >= rect.top &&
    e.clientY <= rect.bottom;
}

watch(
  () => collectionStore.draggingPath,
  (path) => {
    if (path) {
      window.addEventListener('pointermove', onWindowPointerMove);
    } else {
      window.removeEventListener('pointermove', onWindowPointerMove);
      isDragOverCollection.value = false;
    }
  }
);

function buildLoadable(path: string): LoadableTrack | null {
  const data = collectionStore.getLoadable(path);
  if (!data) return null;
  return {
    ...data,
    onBeatOffsetChange: (sec) => collectionStore.updateTrack(path, { beatOffset: sec })
  };
}

function onCollectionDrop(e: Event) {
  const { deckId, path } = (e as CustomEvent<{ deckId: string; path: string }>).detail;
  if (deckId !== props.deck.id) return;
  const loadable = buildLoadable(path);
  if (!loadable) return;
  if (props.deck.loopPlaying) {
    pendingLoad.value = loadable;
    return;
  }
  props.deck.loadTrack(loadable);
}

onMounted(() => window.addEventListener('bm:collection-drop', onCollectionDrop));
onUnmounted(() => {
  window.removeEventListener('bm:collection-drop', onCollectionDrop);
  window.removeEventListener('pointermove', onWindowPointerMove);
});

function onConfirmLoad() {
  const loadable = pendingLoad.value;
  pendingLoad.value = null;
  if (loadable) props.deck.loadTrack(loadable);
}
</script>

<style scoped>
.deck {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  transition: background 0.2s;
}

.deck--playing {
  background: color-mix(in srgb, var(--deck-accent) 4%, transparent);
}

.deck--drag-over {
  outline: 2px dashed var(--deck-accent);
  outline-offset: -4px;
}

.deck__waveform {
  flex: 1;
  width: 100%;
  min-height: 0;
}

.deck__header {
  display: flex;
  align-items: center;
  gap: 0.6em;
  width: 100%;
  padding: 0.5em 0.8em;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.deck__label {
  font-size: 0.9em;
  font-weight: 700;
  letter-spacing: 0.25em;
  color: var(--deck-accent);
  flex-shrink: 0;
}

.deck__status-dot {
  width: 0.5em;
  height: 0.5em;
  border-radius: 50%;
  background: var(--color-border);
  flex-shrink: 0;
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
  flex: 1;
  min-width: 0;
}

.deck__bpm-header {
  display: flex;
  align-items: baseline;
  gap: 0.25em;
  flex-shrink: 0;
  margin-left: auto;
}

.deck__bpm-value-header {
  font-size: 0.85em;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: var(--deck-accent);
  cursor: text;
  letter-spacing: -0.01em;
}

.deck__bpm-input-header {
  font-size: 0.85em;
  font-weight: 700;
  font-family: var(--font);
  font-variant-numeric: tabular-nums;
  background: transparent;
  border: none;
  border-bottom: 1px solid currentColor;
  color: var(--deck-accent);
  width: 5ch;
  padding: 0;
  outline: none;
}

.deck__bpm-unit-header {
  font-size: 0.6em;
  color: var(--color-muted);
  letter-spacing: 0.1em;
}

.deck__drop-zone {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}
.deck__drop-hint {
  color: var(--color-muted);
  font-size: 0.65em;
  letter-spacing: 0.1em;
  opacity: 0.6;
}

.deck__overview {
  padding: 0.5em 0.8em 0;
}

.deck__controls {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: space-around;
  padding: 0.5em 0.8em 0.8em;
  gap: 0.5em;
  min-height: 0;
}

.deck__phase-ring {
  width: 7em;
  height: 7em;
  flex-shrink: 0;
}

.deck__transport-cluster {
  display: flex;
  flex-direction: column;
  gap: 0.5em;
}

.deck__btn-row {
  display: flex;
  gap: 0.5em;
}

.deck__btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.25em;
  width: 5em;
  height: 4em;
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
.deck__btn:disabled {
  opacity: 0.35;
  cursor: default;
}

.deck__btn-key {
  font-size: 0.6em;
  color: var(--color-muted);
  letter-spacing: 0.15em;
}
.deck__btn-icon {
  font-size: 0.85em;
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

.deck__pitch-wrapper {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.2em;
  flex-shrink: 0;
}

.deck__slider-label {
  font-size: 0.6em;
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
</style>
