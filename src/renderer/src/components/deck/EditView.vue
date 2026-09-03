<template>
  <div class="edit-view" :style="{ '--deck-accent': deck.accent }">
    <ConfirmModal
      :open="pendingLoad !== null"
      :title="$t('deck.loadTitle')"
      :body="$t('deck.loadBody')"
      @confirm="confirmPendingLoad"
      @cancel="cancelPendingLoad"
    />
    <BpmModal
      :open="bpmModalOpen"
      :current-bpm="deck.trackBpm"
      @submit="onBpmSubmit"
      @cancel="bpmModalOpen = false"
    />

    <div
      class="edit-view__body"
      ref="deckEl"
      data-deck-id="E"
      :class="{ 'edit-view__body--drag-over': isDragOver }"
    >
      <WaveformDisplay
        class="edit-view__waveform"
        :[DROP_LANDING_ATTRIBUTE]="''"
        :accent="deck.accent"
        :track-data="deck.trackData"
        :loading="deck.loading"
        :track-bpm="deck.trackBpm"
        :beat-offset="deck.beatOffset"
        :cue-point="deck.cuePoint"
        :loop-region="deck.loopRegion"
        :loop-active="deck.loopActive"
        :dense-spectral-data="deck.denseSpectralData"
        :dense-spectral-rate="deck.denseSpectralRate"
        :dense-points-ready="deck.densePointsReady"
        :band-balance="deck.bandBalance"
        :waveform-style="settings.waveformStyle"
        :get-track-position="() => deck.trackPosition"
        :get-playhead-position="deck.getPlayheadPosition"
        :get-spectral-waveform-region="deck.getSpectralWaveformRegion"
        @set-beat-offset="deck.setBeatOffset"
        @seek="deck.seekTo"
      />

      <div class="edit-view__controls">
        <button
          class="edit-view__btn edit-view__btn--play"
          :class="{ 'edit-view__btn--playing': deck.playing }"
          :disabled="!deck.trackLoaded"
          tabindex="-1"
          @click="deck.togglePlay()"
        >
          {{ deck.playing ? '⏸︎' : '▶︎' }}
        </button>
        <button
          class="edit-view__btn edit-view__btn--cue"
          :class="{ 'edit-view__btn--cueing': deck.cueing }"
          :disabled="!deck.trackLoaded"
          tabindex="-1"
          @mousedown.prevent="deck.cueStart()"
          @mouseup="deck.cueEnd()"
          @mouseleave="deck.cueEnd()"
        >
          {{ $t('editView.cue') }}
        </button>
        <span class="edit-view__duration">
          {{ deck.trackData ? formatMs(deck.trackData.duration * 1000) : '--:--' }}
        </span>
        <span
          class="edit-view__filename"
          :class="{ 'edit-view__filename--empty': !deck.trackLoaded }"
          >{{ deck.trackName || $t('deck.notLoaded') }}</span
        >
        <div class="edit-view__controls-right">
          <button
            class="edit-view__btn edit-view__btn--set-bpm"
            :disabled="!deck.trackLoaded"
            tabindex="-1"
            @click="bpmModalOpen = true"
          >
            {{ $t('editView.setBpm') }}
          </button>
          <button
            class="edit-view__btn edit-view__btn--set-grid"
            :disabled="!deck.trackLoaded"
            tabindex="-1"
            @click="onSetGrid()"
          >
            {{ $t('editView.setGrid') }}
          </button>
          <button
            class="edit-view__btn edit-view__btn--eject"
            :disabled="!deck.trackLoaded"
            tabindex="-1"
            @click="deck.ejectTrack()"
          >
            ⏏
          </button>
        </div>
      </div>
    </div>

    <Browser class="edit-view__collection" />
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import type { Deck } from '@renderer/stores/decks';
import { useCollectionStore } from '@renderer/stores/collection';
import { useSettingsStore } from '@renderer/stores/settings';
import { useCollectionDragOver } from '@renderer/composables/useCollectionDragOver';
import { useDeckDrop } from '@renderer/composables/useDeckDrop';
import { formatMs } from '@renderer/utils/time';
import { DROP_LANDING_ATTRIBUTE } from '@renderer/utils/dropLanding';
import WaveformDisplay from '@renderer/components/deck/EditWaveform.vue';
import ConfirmModal from '@renderer/components/modals/ConfirmModal.vue';
import BpmModal from '@renderer/components/modals/BpmModal.vue';
import Browser from '@renderer/components/collection/Browser.vue';

const props = defineProps<{ deck: Deck }>();

// The waveform and its transport, not the whole view: the collection sits beside
// them and is not somewhere a track can be dropped onto this deck.
const deckEl = ref<HTMLElement | null>(null);

const bpmModalOpen = ref(false);

const collectionStore = useCollectionStore();

const settings = useSettingsStore();

const { isDragOver } = useCollectionDragOver(deckEl, () => props.deck.loadedPath);

const { pendingLoad, confirmPendingLoad, cancelPendingLoad } = useDeckDrop({
  deck: () => props.deck,
  resolve: (path) => collectionStore.getLoadableTrack(path)
});

function onSetGrid() {
  props.deck.setBeatOffset(props.deck.getPlayheadPosition());
}

function onBpmSubmit(bpm: number) {
  bpmModalOpen.value = false;
  props.deck.setTrackBpm(bpm);
  if (props.deck.loadedPath) {
    collectionStore.updateTrack(props.deck.loadedPath, { bpm });
  }
}
</script>

<style scoped>
.edit-view {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  overflow: hidden;
  font-family: var(--font);
}

.edit-view__body--drag-over {
  outline: 2px dashed var(--deck-accent);
  outline-offset: -4px;
}

.edit-view__body {
  flex: 0 0 260px;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.edit-view__waveform {
  flex: 1;
  width: 100%;
  min-height: 0;
}

.edit-view__controls {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 4px;
  height: 44px;
  border-top: 1px solid var(--color-border);
  background: #0d0d0d;
  flex-shrink: 0;
}

.edit-view__btn {
  font-family: var(--font);
  font-size: 0.8em;
  /* Pinned so glyphs from fallback fonts (play/pause are not in JetBrains
     Mono) cannot change the button height between states. */
  line-height: 1.2;
  letter-spacing: 0.04em;
  padding: 0.45em 1.2em;
  border-radius: 4px;
  border: 1px solid var(--color-border);
  background: var(--color-surface);
  color: var(--color-muted);
  cursor: pointer;
  text-transform: uppercase;
}

.edit-view__btn--play {
  min-width: 3.6em;
}

.edit-view__btn--set-bpm:hover:not(:disabled),
.edit-view__btn--set-grid:hover:not(:disabled) {
  color: var(--color-text);
  border-color: var(--color-text);
}

.edit-view__duration {
  font-size: 0.8em;
  color: var(--color-muted);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  margin-left: 4px;
}

.edit-view__filename {
  font-size: 0.75em;
  color: var(--color-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 200px;
  opacity: 0.6;
}

.edit-view__filename--empty {
  font-style: italic;
}

.edit-view__btn:disabled {
  opacity: var(--disabled-opacity);
  cursor: default;
}

.edit-view__btn--cue:hover:not(:disabled) {
  color: var(--deck-accent);
  border-color: var(--deck-accent);
}

.edit-view__btn--cueing {
  color: var(--deck-accent);
  border-color: var(--deck-accent);
  background: color-mix(in srgb, var(--deck-accent) 20%, transparent);
}

.edit-view__btn--play:hover:not(:disabled):not(.edit-view__btn--playing) {
  border-color: var(--color-border-hover);
  color: var(--color-text);
  background: var(--toggle-hover-fill);
}

.edit-view__btn--playing {
  background: color-mix(in srgb, var(--deck-accent) var(--toggle-on-fill), transparent);
  border-color: var(--deck-accent);
  color: var(--deck-accent);
}

.edit-view__btn--playing:hover:not(:disabled) {
  background: color-mix(in srgb, var(--deck-accent) var(--toggle-on-fill-hover), transparent);
}

.edit-view__controls-right {
  margin-left: auto;
  display: flex;
  gap: 0.5em;
}

.edit-view__btn--eject:hover:not(:disabled) {
  color: var(--color-text);
  border-color: var(--color-text);
}

.edit-view__collection {
  width: 100%;
  flex: 1;
  min-height: 0;
  border-top: 1px solid var(--color-border);
  overflow: hidden;
  font-size: 13px;
}
</style>
