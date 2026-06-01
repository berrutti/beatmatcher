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
      title="Load new track?"
      body="Playback will stop and the current track will be replaced."
      @confirm="onConfirmLoad"
      @cancel="pendingLoad = null"
    />
    <BpmModal
      :open="bpmModalOpen"
      :current-bpm="deck.trackBpm"
      @submit="onBpmSubmit"
      @cancel="bpmModalOpen = false"
    />

    <div class="edit-view__header">
      <span class="edit-view__track-name" :title="deck.trackName || ''">{{
        deck.trackName || 'No track loaded'
      }}</span>
      <button class="edit-view__close" tabindex="-1" @click="emit('close')">✕</button>
    </div>

    <div v-if="!deck.trackLoaded" class="edit-view__drop-zone">
      <span class="edit-view__drop-hint">Drag a track from the collection</span>
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
        class="edit-view__btn edit-view__btn--set-bpm"
        tabindex="-1"
        @click="bpmModalOpen = true"
      >
        SET BPM
      </button>
      <button class="edit-view__btn edit-view__btn--set-grid" tabindex="-1" @click="onSetGrid()">
        SET GRID
      </button>
      <button
        class="edit-view__btn edit-view__btn--cue"
        :class="{ 'edit-view__btn--cueing': deck.cueing }"
        tabindex="-1"
        @mousedown.prevent="deck.cueStart()"
        @mouseup="deck.cueEnd()"
        @mouseleave="deck.cueEnd()"
      >
        CUE
      </button>
      <button
        class="edit-view__btn edit-view__btn--play"
        :class="{ 'edit-view__btn--playing': deck.playing }"
        tabindex="-1"
        @click="deck.togglePlay()"
      >
        {{ deck.playing ? '⏸' : '▶' }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from 'vue';
import type { Deck, LoadableTrack } from '@renderer/stores/decks';
import { useCollectionStore } from '@renderer/stores/collection';
import WaveformDisplay from '@renderer/components/deck/EditWaveform.vue';
import ConfirmModal from '@renderer/components/modals/ConfirmModal.vue';
import BpmModal from '@renderer/components/modals/BpmModal.vue';

const props = defineProps<{ deck: Deck }>();
const emit = defineEmits<{ close: [] }>();

const viewEl = ref<HTMLElement | null>(null);
const pendingLoad = ref<LoadableTrack | null>(null);
const isDragOver = ref(false);
const bpmModalOpen = ref(false);

const collectionStore = useCollectionStore();

function onWindowPointerMove(e: PointerEvent) {
  if (!viewEl.value) return;
  const rect = viewEl.value.getBoundingClientRect();
  isDragOver.value =
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
      isDragOver.value = false;
    }
  }
);

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
onUnmounted(() => {
  window.removeEventListener('bm:collection-drop', onCollectionDrop);
  window.removeEventListener('pointermove', onWindowPointerMove);
});
</script>

<style scoped>
.edit-view {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  overflow: hidden;
}

.edit-view--drag-over {
  outline: 2px dashed var(--deck-accent);
  outline-offset: -4px;
}

.edit-view__header {
  display: flex;
  align-items: center;
  gap: 0.6em;
  padding: 0 0.8em;
  height: 44px;
  flex-shrink: 0;
  border-bottom: 1px solid var(--color-border);
}

.edit-view__track-name {
  font-size: 0.85em;
  color: var(--color-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  flex: 1;
  min-width: 0;
}

.edit-view__close {
  background: transparent;
  border: none;
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.85em;
  cursor: pointer;
  flex-shrink: 0;
  padding: 0.2em 0.3em;
  line-height: 1;
  opacity: 0.5;
}

.edit-view__close:hover {
  opacity: 1;
  color: var(--color-text);
}

.edit-view__drop-zone {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
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
  letter-spacing: 0.1em;
  padding: 0.45em 1.2em;
  border-radius: 4px;
  border: 1px solid var(--color-border);
  background: var(--color-surface);
  color: var(--color-text);
  cursor: pointer;
}

.edit-view__btn--set-bpm,
.edit-view__btn--set-grid {
  color: var(--color-muted);
}

.edit-view__btn--set-bpm:hover,
.edit-view__btn--set-grid:hover {
  color: var(--color-text);
  border-color: var(--color-text);
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
</style>
