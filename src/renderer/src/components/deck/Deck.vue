<template>
  <div
    ref="deckEl"
    class="deck"
    :data-deck-id="props.deck.id"
    :style="{ '--deck-accent': props.deck.accent }"
    :class="{
      'deck--playing': props.deck.playing,
      'deck--drag-over': isDragOverCollection,
      'deck--loading': props.deck.waveformLoading
    }"
  >
    <ConfirmModal
      :open="pendingLoad !== null"
      :title="$t('deck.loadTitle')"
      :body="$t('deck.loadBody')"
      @confirm="onConfirmLoad"
      @cancel="pendingLoad = null"
    />

    <div class="deck__header">
      <span class="deck__label">{{ $t('deck.label', { id: props.deck.id }) }}</span>
      <div v-if="props.deck.trackName" class="deck__track-info">
        <div class="deck__status-dot" :class="{ 'deck__status-dot--on': props.deck.playing }" />
        <span class="deck__track-name" :title="props.deck.trackName">{{
          props.deck.trackName
        }}</span>
      </div>

      <button
        v-if="props.deck.trackLoaded"
        class="deck__q-btn"
        :class="{ 'deck__q-btn--on': props.deck.quantized }"
        :tabindex="-1"
        @click="props.deck.toggleQuantized()"
      >
        Q
      </button>
      <div v-if="props.deck.trackLoaded" class="deck__bpm-header">
        <div class="deck__bpm-value-wrap" @click="onBpmValueClick">
          <input
            v-if="editingBpm"
            ref="bpmInputEl"
            v-model="bpmInputValue"
            class="deck__bpm-input-header"
            type="number"
            min="20"
            step="0.01"
            @blur="onBpmInputBlur"
            @keydown.enter="onBpmInputBlur"
            @keydown.escape="editingBpm = false"
          />
          <span
            class="deck__bpm-value-header"
            :style="{ visibility: editingBpm ? 'hidden' : 'visible' }"
            >{{ props.deck.targetBpm?.toFixed(2) ?? '--.--' }}</span
          >
        </div>
        <span class="deck__bpm-unit-header">{{ $t('deck.bpm') }}</span>
      </div>
      <button
        v-if="props.deck.trackLoaded"
        class="deck__eject-btn"
        :tabindex="-1"
        :title="$t('deck.ejectTitle')"
        @click="props.deck.ejectTrack()"
      >
        ⏏
      </button>
    </div>

    <div class="deck__loading-bar" />

    <div v-if="!props.deck.trackLoaded" class="deck__drop-zone">
      <span class="deck__drop-hint">{{
        props.deck.loading ? $t('deck.loading') : $t('deck.dragHint')
      }}</span>
    </div>

    <div
      class="deck__content"
      :style="{ visibility: props.deck.trackLoaded ? 'visible' : 'hidden' }"
    >
      <TrackWaveform
        class="deck__overview"
        :class="{ 'deck__overview--waveform-loading': props.deck.waveformLoading }"
        :accent="props.deck.accent"
        :track-data="props.deck.trackData"
        :get-playhead-position="props.deck.getPlayheadPosition"
        :full-spectral-data="props.deck.fullSpectralData"
        :loop-region="props.deck.loopRegion"
        :loop-active="props.deck.loopActive"
        :cue-point="props.deck.cuePoint"
        @seek="props.deck.seekTo"
      />

      <div class="deck__controls">
        <div class="deck__phase-ring">
          <PhaseRing
            :accent="props.deck.accent"
            :get-beat="() => props.deck.beat"
            :cover-art="props.deck.coverArt"
          />
        </div>

        <div class="deck__transport-cluster">
          <div class="deck__btn-row">
            <button
              class="deck__btn deck__btn--nudge"
              :class="{ 'deck__btn--active': props.deck.nudging === 'back' }"
              :disabled="!props.deck.trackLoaded || props.deck.loading"
              :tabindex="-1"
              @mousedown="onNudgeStart('back')"
              @mouseup="props.deck.nudgeEnd()"
              @mouseleave="onNudgeMouseLeave"
            >
              <span class="deck__btn-key" :tabindex="-1">{{ keybindings.NUDGE_BACK }}</span>
              <span class="deck__btn-icon">↶</span>
            </button>
            <button
              class="deck__btn deck__btn--nudge"
              :class="{ 'deck__btn--active': props.deck.nudging === 'forward' }"
              :disabled="!props.deck.trackLoaded || props.deck.loading"
              :tabindex="-1"
              @mousedown="onNudgeStart('forward')"
              @mouseup="props.deck.nudgeEnd()"
              @mouseleave="onNudgeMouseLeave"
            >
              <span class="deck__btn-key">{{ keybindings.NUDGE_FORWARD }}</span>
              <span class="deck__btn-icon">↷</span>
            </button>
          </div>

          <div class="deck__btn-row">
            <button
              class="deck__btn deck__btn--cue"
              :class="{ 'deck__btn--cueing': props.deck.cueing }"
              :disabled="!props.deck.trackLoaded || props.deck.loading"
              :tabindex="-1"
              @mousedown.prevent="props.deck.cueStart()"
              @mouseup="props.deck.cueEnd()"
              @mouseleave="onCueMouseLeave"
            >
              <span class="deck__btn-key">{{ keybindings.CUE }}</span>
              <span>{{ $t('deck.cue') }}</span>
            </button>
            <button
              class="deck__btn deck__btn--play"
              :class="{ 'deck__btn--playing': props.deck.playing }"
              :disabled="!props.deck.trackLoaded || props.deck.loading"
              :tabindex="-1"
              @click="onTogglePlay()"
            >
              <span class="deck__btn-key">{{ keybindings.PLAY }}</span>
              <span>{{ props.deck.playing ? '⏸' : '▶' }}</span>
            </button>
          </div>

          <div class="deck__btn-row">
            <button
              class="deck__btn deck__btn--loop-in"
              :class="{ 'deck__btn--loop-active': props.deck.loopActive }"
              :disabled="!props.deck.trackLoaded || props.deck.loading"
              :tabindex="-1"
              @click="props.deck.setLoopIn()"
            >
              <span class="deck__btn-key">{{ keybindings.LOOP_IN }}</span>
              <span class="deck__btn-icon">{{ $t('deck.loopIn') }}</span>
            </button>
            <button
              class="deck__btn deck__btn--loop-out"
              :class="{ 'deck__btn--loop-active': props.deck.loopActive }"
              :disabled="!props.deck.trackLoaded || props.deck.loading"
              :tabindex="-1"
              @click="onLoopOutClick()"
            >
              <span class="deck__btn-key">{{ keybindings.LOOP_OUT_EXIT }}</span>
              <span class="deck__btn-icon">{{ loopOutLabel() }}</span>
            </button>
          </div>
        </div>

        <div class="deck__pitch-wrapper">
          <span class="deck__slider-label">-{{ settingsStore.pitchRange }}%</span>
          <input
            type="range"
            class="deck__slider"
            :min="-settingsStore.pitchRange"
            :max="settingsStore.pitchRange"
            step="0.01"
            :value="-props.deck.pitchOffset"
            orient="vertical"
            :disabled="!props.deck.trackLoaded || props.deck.loading"
            @input="onSliderInput"
            @dblclick="onPitchDblClick"
          />
          <span class="deck__slider-label">+{{ settingsStore.pitchRange }}%</span>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, nextTick, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { shiftHeld } from '@renderer/composables/useKeyboard';
import { useCollectionDragOver } from '@renderer/composables/useCollectionDragOver';
import type { Deck, LoadableTrack } from '@renderer/stores/decks';
import { useSettingsStore } from '@renderer/stores/settings';
import type { Keybindings } from '@renderer/keybindings';
import { useCollectionStore } from '@renderer/stores/collection';
import PhaseRing from '@renderer/components/deck/PhaseRing.vue';
import TrackWaveform from '@renderer/components/deck/TrackWaveform.vue';
import ConfirmModal from '@renderer/components/modals/ConfirmModal.vue';

const { t } = useI18n();

const PRIMARY_BUTTON = 1;

const deckEl = ref<HTMLElement | null>(null);
const settingsStore = useSettingsStore();

const props = defineProps<{
  deck: Deck;
}>();

const keybindings = computed(() => settingsStore.keybindings[props.deck.id as keyof Keybindings]);

const editingBpm = ref(false);
const bpmInputEl = ref<HTMLInputElement | null>(null);
const bpmInputValue = ref('');

async function startEditingBpm() {
  bpmInputValue.value = props.deck.targetBpm?.toFixed(2) ?? '';
  editingBpm.value = true;
  await nextTick();
  bpmInputEl.value?.select();
}

function onBpmValueClick() {
  if (!props.deck.trackLoaded) return;
  startEditingBpm();
}

function onBpmInputBlur() {
  const val = parseFloat(bpmInputValue.value);
  if (!isNaN(val) && val > 0) props.deck.setTargetBpm(val);
  editingBpm.value = false;
}

// The slider value is negated: like a CDJ pitch fader, up = slower, down = faster.
function onSliderInput(e: Event) {
  if (!props.deck.trackLoaded) return;
  const val = parseFloat((e.target as HTMLInputElement).value);
  props.deck.setPitchOffset(-val);
}

function onPitchDblClick() {
  if (!props.deck.trackLoaded) return;
  props.deck.setPitchOffset(0);
}

function onNudgeStart(direction: 'back' | 'forward') {
  if (!props.deck.trackLoaded) return;
  props.deck.nudgeStart(direction);
}

function loopOutLabel(): string {
  if (shiftHeld.value && props.deck.loopActive) return t('deck.loopExit');
  if (shiftHeld.value && props.deck.loopRegion) return t('deck.loopReloop');
  return t('deck.loopOut');
}

function onLoopOutClick() {
  if (shiftHeld.value && props.deck.loopActive) props.deck.exitLoop();
  else if (shiftHeld.value && props.deck.loopRegion) props.deck.reloop();
  else props.deck.setLoopOut();
}

function onCueMouseLeave(e: MouseEvent) {
  if (e.buttons & PRIMARY_BUTTON) props.deck.cueEnd();
}

function onNudgeMouseLeave(e: MouseEvent) {
  if (e.buttons & PRIMARY_BUTTON) props.deck.nudgeEnd();
}

function onTogglePlay() {
  if (!props.deck.trackLoaded) return;
  props.deck.togglePlay();
}

const pendingLoad = ref<LoadableTrack | null>(null);

const collectionStore = useCollectionStore();
const { isDragOver: isDragOverCollection } = useCollectionDragOver(
  deckEl,
  () => props.deck.loadedPath
);

function onCollectionDrop(e: Event) {
  const { deckId, path } = (e as CustomEvent<{ deckId: string; path: string }>).detail;
  if (deckId !== props.deck.id) return;
  if (props.deck.loadedPath === path) return;
  const loadable = collectionStore.getLoadableTrack(path);
  if (!loadable) return;
  if (props.deck.loopPlaying) {
    pendingLoad.value = loadable;
    return;
  }
  props.deck.loadTrack(loadable);
}

onMounted(() => window.addEventListener('bm:collection-drop', onCollectionDrop));
onUnmounted(() => window.removeEventListener('bm:collection-drop', onCollectionDrop));

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
  position: relative;
}

.deck--playing {
  background: color-mix(in srgb, var(--deck-accent) 4%, transparent);
}

.deck--drag-over {
  outline: 2px dashed var(--deck-accent);
  outline-offset: -4px;
}

.deck__header {
  display: flex;
  align-items: baseline;
  gap: 0.6em;
  width: 100%;
  padding: 0.5em 0.8em;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.deck__track-info {
  display: flex;
  align-items: center;
  gap: 0.4em;
  flex: 1;
  min-width: 0;
  overflow: hidden;
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
  margin-right: 0.3em;
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
  align-items: center;
  gap: 0.25em;
  flex-shrink: 0;
  margin-left: auto;
}

.deck__bpm-value-wrap {
  position: relative;
  cursor: text;
}

.deck__bpm-value-header {
  font-size: 0.85em;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: var(--deck-accent);
  letter-spacing: -0.01em;
  display: block;
}

.deck__bpm-input-header {
  position: absolute;
  inset: 0;
  font-size: 0.85em;
  font-weight: 700;
  font-family: var(--font);
  font-variant-numeric: tabular-nums;
  background: transparent;
  border: none;
  box-shadow: 0 1px 0 0 var(--deck-accent);
  color: var(--deck-accent);
  width: 100%;
  padding: 0;
  outline: none;
  line-height: inherit;
  appearance: textfield;
}
.deck__bpm-input-header::-webkit-inner-spin-button,
.deck__bpm-input-header::-webkit-outer-spin-button {
  display: none;
}
.deck__bpm-input-header::selection {
  background: var(--deck-accent);
  color: var(--color-bg);
}

.deck__bpm-unit-header {
  font-size: 0.6em;
  color: var(--color-muted);
  letter-spacing: 0.1em;
}

.deck__loading-bar {
  height: 2px;
  width: 100%;
  flex-shrink: 0;
  overflow: hidden;
}

.deck__loading-bar::after {
  content: '';
  display: block;
  height: 100%;
  background: linear-gradient(90deg, transparent, var(--deck-accent), transparent);
  transform: translateX(-100%);
}

.deck--loading .deck__loading-bar::after {
  animation: deck-loading-sweep 2s ease-in-out infinite;
}

@keyframes deck-loading-sweep {
  from {
    transform: translateX(-100%);
  }
  to {
    transform: translateX(200%);
  }
}

.deck__drop-zone {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0 1em;
  pointer-events: none;
}

.deck__content {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.deck__drop-hint {
  color: var(--color-muted);
  font-style: italic;
  font-size: 1.2em;
  opacity: 0.6;
  text-align: center;
}

.deck__overview {
  padding: 0.5em 0.8em 0;
  transition: opacity 0.3s ease;
}

.deck__overview--waveform-loading {
  opacity: 0.3;
}

.deck__controls {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: space-evenly;
  min-height: 0;
}

.deck__phase-ring {
  aspect-ratio: 1;
  height: clamp(2rem, 0.5rem + 10cqi, 10rem);
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
  width: 4.5em;
  height: 3.5em;
  border: 1px solid var(--color-border);
  border-radius: var(--radius);
  background: var(--color-surface);
  color: var(--color-text);
  font-family: var(--font);
  font-size: clamp(1em, 0.7cqi, 1.2em);
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
  text-transform: uppercase;
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
.deck__q-btn {
  padding: 0 0.35em;
  height: 1.4em;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: transparent;
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.65em;
  font-weight: 600;
  letter-spacing: 0.05em;
  cursor: pointer;
  flex-shrink: 0;
  line-height: 1;
}
.deck__q-btn--on {
  color: var(--deck-accent);
  border-color: var(--deck-accent);
}

.deck__eject-btn {
  padding: 0 0.3em;
  height: 1.4em;
  border: 1px solid transparent;
  border-radius: 3px;
  background: transparent;
  color: var(--color-muted);
  font-size: 0.75em;
  cursor: pointer;
  flex-shrink: 0;
  line-height: 1;
  opacity: 0.5;
  transition:
    opacity 0.1s,
    color 0.1s;
}
.deck__eject-btn:hover {
  opacity: 1;
  color: var(--color-fg);
}

.deck__btn--loop-in:hover:not(:disabled),
.deck__btn--loop-out:hover:not(:disabled) {
  border-color: var(--deck-accent);
  color: var(--deck-accent);
}
.deck__btn--loop-active {
  border-color: #ca8a04;
  color: var(--color-accent-amber);
  background: color-mix(in srgb, var(--color-accent-amber) 18%, transparent);
}
.deck__pitch-wrapper {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.2em;
  flex-shrink: 0;
}

.deck__slider-label {
  font-size: 0.75em;
  color: var(--color-muted);
}

.deck__slider {
  -webkit-appearance: none;
  appearance: none;
  writing-mode: vertical-lr;
  direction: rtl;
  width: 30px;
  height: 16em;
  cursor: pointer;
  background: transparent;
  padding: 0;
}

.deck__slider::-webkit-slider-runnable-track {
  width: 4px;
  background: #161616;
  border: 1px solid #2c2c2c;
  border-radius: 2px;
}

.deck__slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 28px;
  height: 20px;
  background:
    repeating-linear-gradient(
      to bottom,
      rgba(0, 0, 0, 0.4) 0px,
      rgba(0, 0, 0, 0.4) 1px,
      transparent 1px,
      transparent 3px
    ),
    linear-gradient(to right, #4a4a4a, #858585 25%, #9a9a9a 50%, #858585 75%, #4a4a4a);
  border-radius: 2px;
  border-top: 1px solid #aaa;
  border-bottom: 1px solid #2a2a2a;
  border-left: 1px solid #666;
  border-right: 1px solid #666;
  cursor: grab;
  margin-left: -14px;
  box-shadow: 0 2px 5px rgba(0, 0, 0, 0.7);
}

.deck__slider:disabled {
  opacity: 0.35;
  cursor: default;
}

.deck__slider:disabled::-webkit-slider-thumb {
  cursor: default;
}
</style>
