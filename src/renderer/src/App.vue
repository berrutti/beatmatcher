<template>
  <div class="app" :class="{ 'app--collection-open': collectionStore.isOpen }">
    <Modal
      :open="enterEditPending"
      title="Enter Edit mode?"
      confirm-label="Enter Edit"
      @confirm="onConfirmEditMode"
      @cancel="enterEditPending = false"
    >
      <p class="app__modal-body">
        Playback is running. You can still hear the decks while in Edit mode.
      </p>
    </Modal>

    <TopStrip :edit-mode="editMode" @toggle-edit="tryToggleEditMode" />

    <div class="app__body">
      <EditView v-if="editMode" :deck="store.deckE" @close="editMode = false" />
      <div v-else class="app__play" :class="{ 'app__play--two-deck': mixerStore.deckCount === 2 }">
        <DeckPanel class="app__deck-a" :deck="store.deckA" :keybindings="KEYS.deckA" />
        <DeckPanel
          v-if="mixerStore.deckCount === 4"
          class="app__deck-c"
          :deck="store.deckC"
          :keybindings="KEYS.deckC"
        />
        <div class="app__center">
          <MixerPanel />
        </div>
        <DeckPanel class="app__deck-b" :deck="store.deckB" :keybindings="KEYS.deckB" />
        <DeckPanel
          v-if="mixerStore.deckCount === 4"
          class="app__deck-d"
          :deck="store.deckD"
          :keybindings="KEYS.deckD"
        />
      </div>
    </div>

    <button class="app__collection-bar" @click="collectionStore.toggle()">
      <span class="app__collection-bar-label">COLLECTION</span>
      <span>{{ collectionStore.isOpen ? '▴' : '▾' }}</span>
    </button>
    <CollectionPanel v-show="collectionStore.isOpen" class="app__collection" />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onUnmounted } from 'vue';
import { useDecksStore } from '@renderer/stores/decks';
import { useCollectionStore } from '@renderer/stores/collection';
import { useMixerStore } from '@renderer/stores/mixer';
import { useKeyboard } from '@renderer/composables/useKeyboard';
import { KEYS } from '@renderer/keybindings';
import DeckPanel from '@renderer/components/DeckPanel.vue';
import MixerPanel from '@renderer/components/MixerPanel.vue';
import CollectionPanel from '@renderer/components/CollectionPanel.vue';
import TopStrip from '@renderer/components/TopStrip.vue';
import EditView from '@renderer/components/EditView.vue';
import Modal from '@renderer/components/Modal.vue';

useKeyboard();

const store = useDecksStore();
const collectionStore = useCollectionStore();
const mixerStore = useMixerStore();
onUnmounted(() => store.destroy());

const editMode = computed({
  get: () => store.editMode,
  set: (v) => {
    store.editMode = v;
  }
});
const enterEditPending = ref(false);

function tryToggleEditMode() {
  if (store.editMode) {
    store.editMode = false;
    return;
  }
  if (
    store.deckA.loopPlaying ||
    store.deckB.loopPlaying ||
    store.deckC.loopPlaying ||
    store.deckD.loopPlaying
  ) {
    enterEditPending.value = true;
  } else {
    store.editMode = true;
  }
}

function onConfirmEditMode() {
  enterEditPending.value = false;
  store.editMode = true;
}
</script>

<style scoped>
.app__modal-body {
  font-size: 0.75rem;
  color: var(--color-muted);
  line-height: 1.5;
  margin: 0;
}

.app {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  --topstrip-h: 26px;
  --collection-panel-h: 0px;
  --collection-bar-h: 22px;
}

.app--collection-open {
  --collection-panel-h: 200px;
}

.app__body {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  font-size: clamp(
    11px,
    calc((100dvh - var(--topstrip-h) - var(--collection-bar-h) - var(--collection-panel-h)) / 58),
    15px
  );
}

.app__play {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  grid-template-rows: 1fr 1fr;
  grid-template-areas:
    'deck-a center deck-b'
    'deck-c center deck-d';
}

.app__play--two-deck {
  grid-template-rows: 1fr 1fr;
  grid-template-areas:
    'deck-a center deck-b'
    '.      center .     ';
}

.app__deck-a {
  grid-area: deck-a;
  min-width: 0;
  border-right: 1px solid var(--color-border);
  border-bottom: 1px solid var(--color-border);
}

.app__deck-c {
  grid-area: deck-c;
  min-width: 0;
  border-right: 1px solid var(--color-border);
}

.app__deck-b {
  grid-area: deck-b;
  min-width: 0;
  border-left: 1px solid var(--color-border);
  border-bottom: 1px solid var(--color-border);
}

.app__deck-d {
  grid-area: deck-d;
  min-width: 0;
  border-left: 1px solid var(--color-border);
}

.app__center {
  grid-area: center;
  display: flex;
  flex-direction: column;
  border-left: 1px solid var(--color-border);
  border-right: 1px solid var(--color-border);
}

.app__collection-bar {
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
  font-size: 11px;
  letter-spacing: 0.15em;
  color: var(--color-muted);
  user-select: none;
  flex-shrink: 0;
  border-left: none;
  border-right: none;
  border-bottom: none;
}
.app__collection-bar:hover {
  color: var(--color-text);
  background: var(--color-surface);
}

.app__collection {
  width: 100%;
  height: var(--collection-panel-h);
  flex-shrink: 0;
  overflow: hidden;
}
</style>
