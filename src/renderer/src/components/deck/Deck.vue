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
      @confirm="confirmPendingLoad"
      @cancel="cancelPendingLoad"
    />

    <ConfirmModal
      :open="props.deck.ejectPending"
      :title="$t('deck.ejectConfirmTitle')"
      :body="$t('deck.ejectConfirmBody')"
      :confirm-label="$t('deck.ejectTitle')"
      @confirm="props.deck.confirmEject()"
      @cancel="props.deck.cancelEject()"
    />

    <div class="deck__loading-bar" />

    <div v-if="props.compact" class="deck__compact">
      <button
        class="deck__compact-btn"
        :disabled="!props.deck.trackLoaded || props.deck.loading"
        :tabindex="-1"
        @mousedown.prevent="props.deck.cueStart()"
        @mouseup="props.deck.cueEnd()"
        @mouseleave="onCueMouseLeave"
      >
        {{ $t('deck.cue') }}
      </button>
      <button
        class="deck__compact-btn"
        :class="{ 'deck__compact-btn--playing': props.deck.playing }"
        :disabled="!props.deck.trackLoaded || props.deck.loading"
        :tabindex="-1"
        @click="onTogglePlay()"
      >
        {{ props.deck.playing ? '⏸' : '▶' }}
      </button>

      <div class="deck__compact-info">
        <span class="deck__label" :class="{ 'deck__label--empty': !props.deck.trackLoaded }">{{
          $t('deck.label', { id: props.deck.id })
        }}</span>
        <span
          class="deck__compact-track-name"
          :class="{ 'deck__track-name--empty': !props.deck.trackName }"
          v-tooltip.truncated="props.deck.trackName || undefined"
          >{{ props.deck.trackName || $t('deck.notLoaded') }}</span
        >
      </div>

      <TrackWaveform
        class="deck__compact-overview"
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

      <DeckBpmHeader :deck="props.deck" />
    </div>

    <div v-else class="deck__body">
      <div class="deck__main">
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

        <div class="deck__info-row">
          <div class="deck__phase-ring">
            <PhaseRing
              :accent="props.deck.accent"
              :active="props.deck.trackLoaded"
              :playing="props.deck.playing"
              :cueing="props.deck.cueing"
              :get-beat="() => props.deck.beat"
              :cover-art="props.deck.coverArt"
            />
          </div>

          <div class="deck__info">
            <div class="deck__info-top">
              <span
                class="deck__label"
                :class="{ 'deck__label--empty': !props.deck.trackLoaded }"
                >{{ $t('deck.label', { id: props.deck.id }) }}</span
              >

              <button
                class="deck__q-btn"
                :class="{ 'deck__q-btn--on': props.deck.quantized }"
                :disabled="!props.deck.trackLoaded || !props.deck.hasGrid"
                :tabindex="-1"
                v-tooltip="quantizeTooltip"
                @click="props.deck.toggleQuantized()"
              >
                Q
              </button>
              <button
                class="deck__eject-btn"
                :disabled="!props.deck.trackLoaded"
                :tabindex="-1"
                v-tooltip="props.deck.trackLoaded ? $t('deck.ejectTitle') : undefined"
                @click="props.deck.requestEject()"
              >
                ⏏
              </button>
            </div>

            <div class="deck__track-info">
              <span v-if="!props.deck.trackName" class="deck__track-name--empty-label">{{
                $t('deck.notLoaded')
              }}</span>
              <template v-else>
                <p
                  v-if="artistTitle.artist"
                  class="deck__track-line deck__track-line--artist"
                  v-tooltip.truncated="artistTitle.artist"
                >
                  <span class="deck__track-line-label">{{ $t('deck.artist') }}</span
                  ><span class="deck__track-line-value">{{ artistTitle.artist }}</span>
                </p>
                <p
                  class="deck__track-line deck__track-line--track"
                  v-tooltip.truncated="artistTitle.title ?? undefined"
                >
                  <span class="deck__track-line-label">{{ $t('deck.track') }}</span
                  ><span class="deck__track-line-value">{{ artistTitle.title }}</span>
                </p>
              </template>
            </div>

            <DeckBpmHeader :deck="props.deck" />
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
                <span class="deck__btn-icon">{{ $t('deck.cue') }}</span>
              </button>
              <button
                class="deck__btn deck__btn--play"
                :class="{ 'deck__btn--playing': props.deck.playing }"
                :disabled="!props.deck.trackLoaded || props.deck.loading"
                :tabindex="-1"
                @click="onTogglePlay()"
              >
                <span class="deck__btn-key">{{ keybindings.PLAY }}</span>
                <span class="deck__btn-icon">{{ props.deck.playing ? '⏸' : '▶' }}</span>
              </button>
            </div>

            <div class="deck__btn-row">
              <button
                class="deck__btn deck__btn--loop-in"
                :class="{
                  'deck__btn--loop-active': props.deck.loopActive,
                  'deck__btn--loop-region': props.deck.loopRegion && !props.deck.loopActive
                }"
                :disabled="!props.deck.trackLoaded || props.deck.loading"
                :tabindex="-1"
                @click="props.deck.setLoopIn()"
              >
                <span class="deck__btn-key">{{ keybindings.LOOP_IN }}</span>
                <span class="deck__btn-icon">{{ $t('deck.loopIn') }}</span>
              </button>
              <button
                class="deck__btn deck__btn--loop-out"
                :class="{
                  'deck__btn--loop-active': props.deck.loopActive,
                  'deck__btn--loop-region': props.deck.loopRegion && !props.deck.loopActive
                }"
                :disabled="!props.deck.trackLoaded || props.deck.loading"
                :tabindex="-1"
                @click="onLoopOutClick()"
              >
                <span class="deck__btn-key">{{ keybindings.LOOP_OUT_EXIT }}</span>
                <span class="deck__btn-icon">{{ loopOutLabel() }}</span>
              </button>
            </div>
          </div>
        </div>
      </div>

      <div class="deck__pitch-wrapper">
        <span class="deck__slider-label">-{{ settingsStore.pitchRange }}</span>
        <input
          type="range"
          class="deck__slider"
          :min="-settingsStore.pitchRange"
          :max="settingsStore.pitchRange"
          step="0.01"
          :value="-props.deck.pitchOffset"
          orient="vertical"
          :disabled="!props.deck.trackLoaded || props.deck.loading"
          v-tooltip="props.deck.trackLoaded ? $t('deck.pitchHint') : undefined"
          v-slider-reset="{ enabled: settingsStore.sliderClickResets, reset: onPitchReset }"
          @input="onSliderInput"
          @dblclick="onPitchDblClick"
        />
        <span class="deck__slider-label">+{{ settingsStore.pitchRange }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { shiftHeld } from '@renderer/composables/useKeyboard';
import { useCollectionDragOver } from '@renderer/composables/useCollectionDragOver';
import { useDeckDrop } from '@renderer/composables/useDeckDrop';
import type { Deck } from '@renderer/stores/decks';
import { useSettingsStore } from '@renderer/stores/settings';
import type { Keybindings } from '@renderer/keybindings';
import { useCollectionStore } from '@renderer/stores/collection';
import PhaseRing from '@renderer/components/deck/PhaseRing.vue';
import TrackWaveform from '@renderer/components/deck/TrackWaveform.vue';
import DeckBpmHeader from '@renderer/components/deck/DeckBpmHeader.vue';
import ConfirmModal from '@renderer/components/modals/ConfirmModal.vue';

const { t } = useI18n();

const PRIMARY_BUTTON = 1;

const deckEl = ref<HTMLElement | null>(null);
const settingsStore = useSettingsStore();

const props = defineProps<{
  deck: Deck;
  compact?: boolean;
}>();

const keybindings = computed(() => settingsStore.keybindings[props.deck.id as keyof Keybindings]);

// Explains the disablement when the reason is not the empty deck: quantizing
// snaps to a grid, and a track whose BPM never resolved has none.
const quantizeTooltip = computed(() => {
  if (!props.deck.trackLoaded) return undefined;
  return props.deck.hasGrid ? t('deck.quantizeTitle') : t('deck.quantizeNeedsGrid');
});

// Track metadata only stores a single combined "Artist - Title" (or "Artist
// – Title") string, not separate fields, so split it here for display.
const ARTIST_TITLE_SEPARATOR = /\s[–-]\s/;
const artistTitle = computed(() => {
  const name = props.deck.trackName;
  if (!name) return { artist: null, title: null };
  const match = name.split(ARTIST_TITLE_SEPARATOR);
  if (match.length < 2) return { artist: null, title: name };
  return { artist: match[0], title: match.slice(1).join(' - ') };
});

// Up is slower, down is faster.
async function onSliderInput(event: Event) {
  if (!props.deck.trackLoaded) return;
  const target = event.target;
  if (!(target instanceof HTMLInputElement)) return;
  await props.deck.setPitchOffset(-parseFloat(target.value));
}

async function onPitchDblClick() {
  if (!props.deck.trackLoaded) return;
  await props.deck.setPitchOffset(0);
}

// The directive's reset returns nothing, so the rejection is caught here rather
// than left floating.
function onPitchReset(): void {
  onPitchDblClick().catch(() => {});
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

const collectionStore = useCollectionStore();
const { isDragOver: isDragOverCollection } = useCollectionDragOver(
  deckEl,
  () => props.deck.loadedPath
);

const { pendingLoad, confirmPendingLoad, cancelPendingLoad } = useDeckDrop({
  deck: () => props.deck,
  resolve: (path) => collectionStore.getLoadableTrack(path)
});
</script>

<style scoped>
.deck {
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  transition: background 0.2s;
  position: relative;
  /* Shared by .deck__btn and .deck__compact-btn's plain hover state. */
  --btn-hover-border: #444;
  --btn-hover-bg: #1e1e1e;
}

.deck--playing {
  background: color-mix(in srgb, var(--deck-accent) 4%, transparent);
}

.deck--drag-over {
  outline: 2px dashed var(--deck-accent);
  outline-offset: -4px;
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

/* Deck label + BPM readout
   Shared between the normal and compact layouts. */
.deck__label {
  font-size: 0.8em;
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--deck-accent);
  flex-shrink: 0;
}

.deck__label--empty {
  color: var(--color-muted);
  opacity: 0.6;
}

.deck__body {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: stretch;
  gap: 4px;
  padding: 4px;
}

.deck__main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 0.3em;
}

.deck__overview {
  flex-shrink: 0;
  transition: opacity 0.3s ease;
}

.deck__overview--waveform-loading {
  opacity: 0.3;
}

.deck__info-row {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: stretch;
  gap: 0.5em;
}

.deck__phase-ring {
  aspect-ratio: 1;
  height: 100%;
  flex-shrink: 0;
  /* Matches the waveform's SIDE_MARGIN in TrackWaveform.vue, so the ring
     lines up with where the waveform itself starts, not the canvas edge. */
  margin-left: 4px;
}

.deck__info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 0.4em;
}

.deck__info-top {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
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
  letter-spacing: 0.02em;
  cursor: pointer;
  flex-shrink: 0;
  line-height: 1;
}
.deck__q-btn:hover:not(:disabled):not(.deck__q-btn--on) {
  border-color: var(--color-border-hover);
  color: var(--color-text);
  background: var(--toggle-hover-fill);
}
.deck__q-btn--on {
  color: var(--deck-accent);
  border-color: var(--deck-accent);
  background: color-mix(in srgb, var(--deck-accent) var(--toggle-on-fill), transparent);
}
.deck__q-btn--on:hover:not(:disabled) {
  background: color-mix(in srgb, var(--deck-accent) var(--toggle-on-fill-hover), transparent);
}
.deck__q-btn:disabled {
  opacity: var(--disabled-opacity);
  cursor: default;
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
.deck__eject-btn:hover:not(:disabled) {
  opacity: 1;
  color: var(--color-fg);
}
.deck__eject-btn:disabled {
  opacity: 0.2;
  cursor: default;
}

/* Artist / track lines */
.deck__track-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: flex-start;
  gap: 0;
  min-width: 0;
  overflow: hidden;
}

.deck__track-name--empty-label {
  font-size: 0.8em;
  color: var(--color-muted);
  font-style: italic;
  opacity: 0.6;
}

.deck__track-line {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  overflow: hidden;
  word-break: break-word;
  min-width: 0;
  margin: 0;
  font-size: 0.8em;
}

.deck__track-line--artist {
  flex-shrink: 0;
  -webkit-line-clamp: 1;
}

.deck__track-line--track {
  flex: 1;
  -webkit-line-clamp: 3;
}

.deck__track-line-label {
  color: var(--color-muted);
  margin-right: 0.2em;
}

.deck__track-line-value {
  color: var(--color-text);
}

/* 2 columns x 3 rows of (near-)square buttons: shaping the whole cluster's
   box with this ratio (the same height:100%+aspect-ratio trick already used
   by .deck__phase-ring next to it) sizes it correctly up front, so the
   buttons inside can just fill their row with flex:1 without fighting
   .deck__info's greedy flex:1 for leftover width. */
.deck__transport-cluster {
  flex-shrink: 0;
  aspect-ratio: 2 / 3;
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.deck__btn-row {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 4px;
}

.deck__btn {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.25em;
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
.deck__btn:disabled {
  opacity: var(--disabled-opacity);
  cursor: default;
}
/* Generic hover only applies when no button-specific state below is active,
   so a state color (nudge/cue/play/loop) is never masked by this grey hover -
   no specificity fight or !important needed. */
.deck__btn:hover:not(
    :disabled,
    .deck__btn--active,
    .deck__btn--cueing,
    .deck__btn--playing,
    .deck__btn--loop-active,
    .deck__btn--loop-region
  ) {
  border-color: var(--btn-hover-border);
  background: var(--btn-hover-bg);
}

.deck__btn-key {
  font-size: 0.75em;
  font-weight: 600;
  color: var(--color-muted);
  letter-spacing: 0.02em;
  text-transform: uppercase;
}
.deck__btn-icon {
  font-size: 0.75em;
  font-weight: 600;
}

/* Nudge: colored only while actively held (mousedown/keydown) */
.deck__btn--nudge.deck__btn--active {
  background: color-mix(in srgb, var(--color-nudge) 20%, transparent);
  border-color: var(--color-nudge);
  box-shadow: 0 0 0.8em color-mix(in srgb, var(--color-nudge) 30%, transparent);
  color: var(--color-nudge);
}

/* Cue: a hint color on hover, a stronger color while actively held */
.deck__btn--cue:hover:not(:disabled, .deck__btn--cueing) {
  border-color: var(--color-cue);
  color: var(--color-cue);
}
.deck__btn--cueing {
  border-color: var(--color-cue);
  color: var(--color-cue);
  background: color-mix(in srgb, var(--color-cue) 12%, transparent);
}

/* Play: a hint color (the deck's own accent) on hover, green while playing */
.deck__btn--play:hover:not(:disabled, .deck__btn--playing) {
  border-color: var(--deck-accent);
  color: var(--deck-accent);
}
.deck__btn--play.deck__btn--playing {
  border-color: var(--color-play);
  color: var(--color-play);
  background: color-mix(in srgb, var(--color-play) 8%, transparent);
}

/* Loop in/out: these are instant, non-holdable actions, so there is no
   "currently held" hover hint. Cyan marks a defined-but-inactive loop region;
   amber marks an actively looping region. */
.deck__btn--loop-region {
  border-color: var(--color-accent-cyan);
  color: var(--color-accent-cyan);
  background: color-mix(in srgb, var(--color-accent-cyan) 12%, transparent);
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
  height: 100%;
  flex-shrink: 0;
}

.deck__slider-label {
  flex-shrink: 0;
  font-size: 0.6em;
  color: var(--color-muted);
  pointer-events: none;
}

.deck__slider {
  --slider-thumb-length: 0.9em;
  -webkit-appearance: none;
  appearance: none;
  writing-mode: vertical-lr;
  direction: rtl;
  width: 1.4em;
  flex: 1;
  min-height: 0;
  cursor: pointer;
  background: transparent;
  padding: 0;
}

.deck__slider::-webkit-slider-runnable-track {
  width: 2px;
  background-color: #161616;
  background-image: repeating-linear-gradient(
    to bottom,
    #333 0,
    #333 1px,
    transparent 1px,
    transparent 10px
  );
  border-radius: 1px;
}

.deck__slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 1.4em;
  height: var(--slider-thumb-length);
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
  margin-left: -0.7em;
  box-shadow: 0 2px 5px rgba(0, 0, 0, 0.7);
}

.deck__slider:disabled {
  opacity: var(--disabled-opacity);
  cursor: default;
}

.deck__slider:disabled::-webkit-slider-thumb {
  cursor: default;
}

.deck__compact {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px;
}

.deck__compact-btn {
  flex-shrink: 0;
  width: 3.6em;
  height: 1.8em;
  border: 1px solid var(--color-border);
  border-radius: var(--radius);
  background: var(--color-surface);
  color: var(--color-text);
  font-family: var(--font);
  font-size: 0.75em;
  cursor: pointer;
}
.deck__compact-btn:hover:not(:disabled) {
  border-color: var(--btn-hover-border);
  background: var(--btn-hover-bg);
}
.deck__compact-btn:disabled {
  opacity: var(--disabled-opacity);
  cursor: default;
}
.deck__compact-btn--playing {
  border-color: var(--color-play);
  color: var(--color-play);
  background: color-mix(in srgb, var(--color-play) 8%, transparent);
}

.deck__compact-info {
  flex-shrink: 0;
  width: 12em;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 0.15em;
}

.deck__compact-track-name {
  font-size: 0.65em;
  color: var(--color-muted);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.deck__track-name--empty {
  font-style: italic;
  opacity: 0.6;
}

.deck__compact-overview {
  flex: 1;
  min-width: 0;
}
</style>
