<template>
  <div
    class="edit-view"
    ref="viewEl"
    data-deck-id="E"
    :style="{ '--deck-accent': deck.accent }"
    :class="{ 'edit-view--drag-over': isDragOver }"
  >
    <ConfirmModal
      :open="pendingLoad !== null"
      :title="$t('deck.loadTitle')"
      :body="$t('deck.loadBody')"
      @confirm="onConfirmLoad"
      @cancel="pendingLoad = null"
    />
    <BpmModal
      :open="bpmModalOpen"
      :current-bpm="deck.trackBpm"
      @submit="onBpmSubmit"
      @cancel="bpmModalOpen = false"
    />

    <div v-if="!deck.trackLoaded" class="edit-view__drop-zone">
      <span class="edit-view__drop-hint">{{ $t('editView.dropHint') }}</span>
    </div>

    <WaveformDisplay
      v-if="deck.trackLoaded"
      class="edit-view__waveform"
      :accent="deck.accent"
      :track-data="deck.trackData"
      :is-drag-over="isDragOver"
      :track-bpm="deck.trackBpm"
      :beat-offset="deck.beatOffset"
      :cue-point="deck.cuePoint"
      :loop-region="deck.loopRegion"
      :loop-active="deck.loopActive"
      :dense-spectral-data="deck.denseSpectralData"
      :dense-spectral-rate="deck.denseSpectralRate"
      :get-track-position="() => deck.trackPosition"
      :get-playhead-position="deck.getPlayheadPosition"
      :get-spectral-waveform-region="deck.getSpectralWaveformRegion"
      @set-beat-offset="deck.setBeatOffset"
      @seek="deck.seekTo"
    />

    <div v-if="deck.trackLoaded" class="edit-view__controls">
      <button
        class="edit-view__btn edit-view__btn--play"
        :class="{ 'edit-view__btn--playing': deck.playing }"
        tabindex="-1"
        @click="deck.togglePlay()"
      >
        {{ deck.playing ? '⏸︎' : '▶︎' }}
      </button>
      <button
        class="edit-view__btn edit-view__btn--cue"
        :class="{ 'edit-view__btn--cueing': deck.cueing }"
        tabindex="-1"
        @mousedown.prevent="deck.cueStart()"
        @mouseup="deck.cueEnd()"
        @mouseleave="deck.cueEnd()"
      >
        {{ $t('editView.cue') }}
      </button>
      <span v-if="deck.trackData" class="edit-view__duration">
        {{ formatMs(deck.trackData.duration * 1000) }}
      </span>
      <span class="edit-view__filename">{{ deck.trackName }}</span>
      <div class="edit-view__controls-right">
        <button
          class="edit-view__btn edit-view__btn--set-bpm"
          tabindex="-1"
          @click="bpmModalOpen = true"
        >
          {{ $t('editView.setBpm') }}
        </button>
        <button class="edit-view__btn edit-view__btn--set-grid" tabindex="-1" @click="onSetGrid()">
          {{ $t('editView.setGrid') }}
        </button>
        <button
          class="edit-view__btn edit-view__btn--eject"
          tabindex="-1"
          @click="deck.ejectTrack()"
        >
          ⏏
        </button>
      </div>
    </div>

    <button class="edit-view__collection-bar" @click="collectionStore.toggle()">
      <span>{{ $t('editView.collection') }}</span>
      <span>{{ collectionStore.isOpen ? '▾' : '▴' }}</span>
    </button>
    <div
      v-if="collectionStore.isOpen"
      class="edit-view__resize-handle"
      @pointerdown.prevent="onResizeStart"
    />
    <Browser
      v-show="collectionStore.isOpen"
      class="edit-view__collection"
      :style="{ height: collectionStore.isOpen ? collectionHeight + 'px' : '0px' }"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import type { Deck, LoadableTrack } from '@renderer/stores/decks';
import { useCollectionStore } from '@renderer/stores/collection';
import { useCollectionDragOver } from '@renderer/composables/useCollectionDragOver';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import { formatMs } from '@renderer/utils/time';
import WaveformDisplay from '@renderer/components/deck/EditWaveform.vue';
import ConfirmModal from '@renderer/components/modals/ConfirmModal.vue';
import BpmModal from '@renderer/components/modals/BpmModal.vue';
import Browser from '@renderer/components/collection/Browser.vue';

const props = defineProps<{ deck: Deck }>();

const viewEl = ref<HTMLElement | null>(null);
const pendingLoad = ref<LoadableTrack | null>(null);
const bpmModalOpen = ref(false);

const collectionStore = useCollectionStore();
const collectionHeight = ref(storageGet<number>(STORAGE_KEYS.collectionHeight, 200));

const MIN_COLLECTION_H = 120;
const MAX_COLLECTION_H_RATIO = 0.65;

function onResizeStart(e: PointerEvent) {
  const startY = e.clientY;
  const startHeight = collectionHeight.value;

  function onMove(ev: PointerEvent) {
    const delta = startY - ev.clientY;
    const maxH = Math.floor(window.innerHeight * MAX_COLLECTION_H_RATIO);
    collectionHeight.value = Math.max(MIN_COLLECTION_H, Math.min(maxH, startHeight + delta));
  }

  function onUp() {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    storageSet(STORAGE_KEYS.collectionHeight, collectionHeight.value);
  }

  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onUp);
}

const { isDragOver } = useCollectionDragOver(viewEl);

function onSetGrid() {
  props.deck.setBeatOffset(props.deck.getPlayheadPosition());
}

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

function onConfirmLoad() {
  const loadable = pendingLoad.value;
  pendingLoad.value = null;
  if (loadable) props.deck.loadTrack(loadable);
}

function onBpmSubmit(bpm: number) {
  bpmModalOpen.value = false;
  props.deck.setTrackBpm(bpm);
  if (props.deck.loadedPath) {
    collectionStore.updateTrack(props.deck.loadedPath, { bpm });
  }
}

onMounted(() => window.addEventListener('bm:collection-drop', onCollectionDrop));
onUnmounted(() => window.removeEventListener('bm:collection-drop', onCollectionDrop));
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

.edit-view--drag-over {
  outline: 2px dashed var(--deck-accent);
  outline-offset: -4px;
}

.edit-view__drop-zone {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 0;
}

.edit-view__drop-hint {
  font-size: 1em;
  color: var(--color-muted);
  letter-spacing: 0.1em;
  opacity: 0.6;
  font-style: italic;
}

.edit-view__waveform {
  flex: 1;
  width: 100%;
  min-height: 0;
}

.edit-view__controls {
  display: flex;
  align-items: center;
  gap: 0.5em;
  padding: 0 12px;
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
  letter-spacing: 0.1em;
  padding: 0.45em 1.2em;
  border-radius: 4px;
  border: 1px solid var(--color-border);
  background: var(--color-surface);
  color: var(--color-muted);
  cursor: pointer;
}

.edit-view__btn--play {
  min-width: 3.6em;
}

.edit-view__btn--set-bpm:hover,
.edit-view__btn--set-grid:hover {
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

.edit-view__btn--cue:hover {
  color: var(--deck-accent);
  border-color: var(--deck-accent);
}

.edit-view__btn--cueing {
  color: var(--deck-accent);
  border-color: var(--deck-accent);
  background: color-mix(in srgb, var(--deck-accent) 20%, transparent);
}

.edit-view__btn--play:hover,
.edit-view__btn--playing {
  background: color-mix(in srgb, var(--deck-accent) 15%, transparent);
  border-color: var(--deck-accent);
  color: var(--deck-accent);
}

.edit-view__controls-right {
  margin-left: auto;
  display: flex;
  gap: 0.5em;
}

.edit-view__btn--eject:hover {
  color: var(--color-text);
  border-color: var(--color-text);
}

.edit-view__collection-bar {
  width: 100%;
  height: var(--collection-bar-h);
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 0.5em;
  cursor: pointer;
  border-top: 1px solid var(--color-border);
  background: var(--color-bg);
  font-family: var(--font);
  font-size: clamp(10px, 1vw, 12px);
  letter-spacing: 0.15em;
  color: var(--color-muted);
  user-select: none;
  flex-shrink: 0;
  border-left: none;
  border-right: none;
  border-bottom: none;
}

.edit-view__collection-bar:hover {
  color: var(--color-text);
  background: var(--color-surface);
}

.edit-view__resize-handle {
  height: 4px;
  flex-shrink: 0;
  cursor: ns-resize;
  background: var(--color-border);
  opacity: 0.4;
  transition: opacity 0.15s;
}

.edit-view__resize-handle:hover {
  opacity: 0.9;
}

.edit-view__collection {
  flex-shrink: 0;
  overflow: hidden;
  font-size: clamp(11px, 1.1vw, 14px);
}
</style>
