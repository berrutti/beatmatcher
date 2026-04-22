<template>
  <div class="edit-view" ref="viewEl" data-deck-id="E" :style="{ '--deck-accent': deck.accent }">
    <ConfirmModal
      :open="pendingLoad !== null"
      title="Load new track?"
      body="Playback will stop and the current track will be replaced."
      @confirm="onConfirmLoad"
      @cancel="pendingLoad = null"
    />

    <div class="edit-view__header">
      <span class="edit-view__label">EDIT</span>
      <span v-if="deck.trackName" class="edit-view__track-name" :title="deck.trackName">{{
        deck.trackName
      }}</span>
      <div v-if="deck.trackLoaded" class="edit-view__bpm">
        <span class="edit-view__bpm-value">{{ deck.targetBpm?.toFixed(1) ?? '--.-' }}</span>
        <span class="edit-view__bpm-unit">BPM</span>
      </div>
      <div class="edit-view__transport">
        <button
          class="edit-view__btn edit-view__btn--cue"
          :class="{ 'edit-view__btn--cueing': deck.cueing }"
          :disabled="!deck.trackLoaded"
          @mousedown.prevent="deck.cueStart()"
          @mouseup="deck.cueEnd()"
          @mouseleave="deck.cueEnd()"
        >
          CUE
        </button>
        <button
          class="edit-view__btn edit-view__btn--play"
          :class="{ 'edit-view__btn--playing': deck.playing }"
          :disabled="!deck.trackLoaded"
          @click="deck.togglePlay()"
        >
          {{ deck.playing ? '⏸' : '▶' }}
        </button>
      </div>
      <button class="edit-view__close" @click="emit('close')">CLOSE</button>
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
      :dense-spectral-data="deck.denseSpectralData"
      :dense-spectral-rate="deck.denseSpectralRate"
      :get-track-position="() => deck.trackPosition"
      :get-playhead-position="deck.getPlayheadPosition"
      :get-spectral-waveform-region="deck.getSpectralWaveformRegion"
      @set-beat-offset="deck.setBeatOffset"
      @seek="deck.seekTo"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from 'vue';
import type { Deck, LoadableTrack } from '@renderer/stores/decks';
import { useCollectionStore } from '@renderer/stores/collection';
import WaveformDisplay from '@renderer/components/WaveformDisplay.vue';
import ConfirmModal from '@renderer/components/ConfirmModal.vue';

const props = defineProps<{ deck: Deck }>();
const emit = defineEmits<{ close: [] }>();

const viewEl = ref<HTMLElement | null>(null);
const pendingLoad = ref<LoadableTrack | null>(null);
const isDragOver = ref(false);

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

function onConfirmLoad() {
  const loadable = pendingLoad.value;
  pendingLoad.value = null;
  if (loadable) props.deck.loadTrack(loadable);
}

onMounted(() => window.addEventListener('bm:collection-drop', onCollectionDrop));
onUnmounted(() => {
  window.removeEventListener('bm:collection-drop', onCollectionDrop);
  window.removeEventListener('pointermove', onWindowPointerMove);
});
</script>

<style scoped>
.edit-view {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: stretch;
}

.edit-view__header {
  display: flex;
  align-items: center;
  gap: 0.6em;
  padding: 0 0.8em;
  height: 2.2em;
  flex-shrink: 0;
  border-bottom: 1px solid var(--color-border);
}

.edit-view__label {
  font-size: 0.6em;
  font-weight: 700;
  letter-spacing: 0.15em;
  color: var(--deck-accent);
}

.edit-view__track-name {
  font-size: 0.6em;
  color: var(--color-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  flex: 1;
  min-width: 0;
}

.edit-view__bpm {
  display: flex;
  align-items: baseline;
  gap: 0.2em;
  flex-shrink: 0;
}

.edit-view__bpm-value {
  font-size: 0.75em;
  font-variant-numeric: tabular-nums;
  color: var(--color-text);
}

.edit-view__bpm-unit {
  font-size: 0.5em;
  color: var(--color-muted);
  letter-spacing: 0.1em;
}

.edit-view__close {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.55em;
  letter-spacing: 0.12em;
  padding: 0.2em 0.6em;
  border-radius: 3px;
  cursor: pointer;
  flex-shrink: 0;
}

.edit-view__close:hover {
  border-color: var(--color-text);
  color: var(--color-text);
}

.edit-view__drop-zone {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}

.edit-view__drop-hint {
  font-size: 0.6em;
  color: var(--color-muted);
  letter-spacing: 0.1em;
}

.edit-view__waveform {
  flex: 1;
  width: 100%;
  min-height: 0;
}

.edit-view__transport {
  display: flex;
  align-items: center;
  gap: 0.4em;
  flex-shrink: 0;
  margin-left: auto;
}

.edit-view__btn {
  font-family: var(--font);
  font-size: 0.65em;
  letter-spacing: 0.1em;
  padding: 0.4em 1.2em;
  border-radius: 4px;
  border: 1px solid var(--color-border);
  background: var(--color-surface);
  color: var(--color-text);
  cursor: pointer;
}

.edit-view__btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.edit-view__btn--cue {
  color: var(--deck-accent);
  border-color: var(--deck-accent);
}

.edit-view__btn--cueing {
  background: color-mix(in srgb, var(--deck-accent) 20%, transparent);
}

.edit-view__btn--play:hover,
.edit-view__btn--playing {
  background: color-mix(in srgb, var(--deck-accent) 15%, transparent);
  border-color: var(--deck-accent);
  color: var(--deck-accent);
}
</style>
