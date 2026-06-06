<template>
  <div
    class="perf"
    :class="{ 'perf--collection-open': collectionStore.isOpen }"
    :style="{ '--collection-panel-h': collectionStore.isOpen ? collectionHeight + 'px' : '0px' }"
  >
    <div class="perf__body">
      <div class="perf__play" :class="{ 'perf__play--two-deck': mixerStore.deckCount === 2 }">
        <Deck class="perf__deck-a" :deck="decksStore.deckA" />
        <Deck v-if="mixerStore.deckCount === 4" class="perf__deck-c" :deck="decksStore.deckC" />
        <div class="perf__center">
          <Mixer />
        </div>
        <Deck class="perf__deck-b" :deck="decksStore.deckB" />
        <Deck v-if="mixerStore.deckCount === 4" class="perf__deck-d" :deck="decksStore.deckD" />
      </div>
    </div>

    <button class="perf__collection-bar" @click="collectionStore.toggle()">
      <span class="perf__collection-bar-label">COLLECTION</span>
      <span>{{ collectionStore.isOpen ? '▾' : '▴' }}</span>
    </button>
    <div
      v-if="collectionStore.isOpen"
      class="perf__collection-resize-handle"
      @pointerdown.prevent="onResizeStart"
    />
    <Browser v-show="collectionStore.isOpen" class="perf__collection" />
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import { useDecksStore } from '@renderer/stores/decks';
import { useCollectionStore } from '@renderer/stores/collection';
import { useMixerStore } from '@renderer/stores/mixer';
import Deck from '@renderer/components/deck/Deck.vue';
import Mixer from '@renderer/components/mixer/Mixer.vue';
import Browser from '@renderer/components/collection/Browser.vue';

const decksStore = useDecksStore();
const collectionStore = useCollectionStore();
const mixerStore = useMixerStore();

const collectionHeight = ref(storageGet<number>(STORAGE_KEYS.collectionHeight, 200));

function onResizeStart(e: PointerEvent) {
  const startY = e.clientY;
  const startHeight = collectionHeight.value;

  function onMove(ev: PointerEvent) {
    const delta = startY - ev.clientY;
    const maxH = Math.floor(window.innerHeight * 0.65);
    collectionHeight.value = Math.max(120, Math.min(maxH, startHeight + delta));
  }

  function onUp() {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    storageSet(STORAGE_KEYS.collectionHeight, collectionHeight.value);
  }

  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onUp);
}
</script>

<style scoped>
.perf {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  --collection-panel-h: 0px;
  --collection-bar-h: 22px;
}

.perf__body {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  font-size: clamp(
    11px,
    calc(
      (
          100dvh - var(--appbar-h, 0px) - var(--topstrip-h) - var(--collection-bar-h) -
            var(--collection-panel-h)
        ) /
        54
    ),
    15px
  );
}

.perf__play {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  grid-template-rows: 1fr 1fr;
  grid-template-areas:
    'deck-a center deck-b'
    'deck-c center deck-d';
}

.perf__play--two-deck {
  grid-template-rows: 1fr 1fr;
  grid-template-areas:
    'deck-a center deck-b'
    '.      center .     ';
}

.perf__deck-a {
  grid-area: deck-a;
  min-width: 0;
  border-right: 1px solid var(--color-border);
  border-bottom: 1px solid var(--color-border);
}

.perf__deck-c {
  grid-area: deck-c;
  min-width: 0;
  border-right: 1px solid var(--color-border);
}

.perf__deck-b {
  grid-area: deck-b;
  min-width: 0;
  border-left: 1px solid var(--color-border);
  border-bottom: 1px solid var(--color-border);
}

.perf__deck-d {
  grid-area: deck-d;
  min-width: 0;
  border-left: 1px solid var(--color-border);
}

.perf__center {
  grid-area: center;
  display: flex;
  flex-direction: column;
  border-left: 1px solid var(--color-border);
  border-right: 1px solid var(--color-border);
}

.perf__collection-bar {
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

.perf__collection-bar:hover {
  color: var(--color-text);
  background: var(--color-surface);
}

.perf__collection-resize-handle {
  height: 4px;
  flex-shrink: 0;
  cursor: ns-resize;
  background: var(--color-border);
  opacity: 0.4;
  transition: opacity 0.15s;
}

.perf__collection-resize-handle:hover {
  opacity: 0.9;
}

.perf__collection {
  width: 100%;
  height: var(--collection-panel-h);
  flex-shrink: 0;
  overflow: hidden;
  font-size: clamp(11px, 1.1vw, 14px);
}

.perf__modal-body {
  font-size: 0.75rem;
  color: var(--color-muted);
  line-height: 1.5;
  margin: 0;
}
</style>
