<template>
  <div
    class="app"
    :class="{ 'app--collection-open': collectionStore.isOpen }"
    :style="{ '--collection-panel-h': collectionStore.isOpen ? collectionHeight + 'px' : '0px' }"
  >
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

    <Modal
      :open="enterSessionPending"
      title="Enter Session view?"
      confirm-label="Enter Session"
      @confirm="onConfirmEnterSession"
      @cancel="enterSessionPending = false"
    >
      <p class="app__modal-body">
        All loaded tracks will be unloaded. You can reload them when you return to Performance mode.
      </p>
    </Modal>

    <Modal
      :open="exitSessionPending"
      title="Exit Session view?"
      confirm-label="Exit Session"
      @confirm="onConfirmExitSession"
      @cancel="exitSessionPending = false"
    >
      <p class="app__modal-body">
        Your current session will be closed and playback will stop.
      </p>
    </Modal>

    <TopStrip
      :edit-mode="decksStore.editMode"
      @toggle-edit="tryToggleEditMode"
      @open-settings="settingsStore.isOpen = true"
    />
    <SettingsModal v-if="settingsStore.isOpen" />

    <div class="app__body">
      <SessionView v-if="sessionStore.sessionMode" @close="tryExitSession" />
      <EditView
        v-else-if="decksStore.editMode"
        :deck="decksStore.deckE"
        @close="decksStore.exitEditMode()"
      />
      <div v-else class="app__play" :class="{ 'app__play--two-deck': mixerStore.deckCount === 2 }">
        <Deck class="app__deck-a" :deck="decksStore.deckA" />
        <Deck v-if="mixerStore.deckCount === 4" class="app__deck-c" :deck="decksStore.deckC" />
        <div class="app__center">
          <Mixer />
        </div>
        <Deck class="app__deck-b" :deck="decksStore.deckB" />
        <Deck v-if="mixerStore.deckCount === 4" class="app__deck-d" :deck="decksStore.deckD" />
      </div>
    </div>

    <div v-if="!sessionStore.sessionMode && !decksStore.editMode" class="app__session-entry">
      <button class="app__session-btn" @click="tryEnterSession">SESSION VIEW</button>
    </div>

    <button class="app__collection-bar" @click="collectionStore.toggle()">
      <span class="app__collection-bar-label">COLLECTION</span>
      <span>{{ collectionStore.isOpen ? '▾' : '▴' }}</span>
    </button>
    <div
      v-if="collectionStore.isOpen"
      class="app__collection-resize-handle"
      @pointerdown.prevent="onResizeStart"
    />
    <Browser v-show="collectionStore.isOpen" class="app__collection" />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import { useDecksStore } from '@renderer/stores/decks';
import { useCollectionStore } from '@renderer/stores/collection';
import { useMixerStore } from '@renderer/stores/mixer';
import { useKeyboard } from '@renderer/composables/useKeyboard';
import { useSettingsStore } from '@renderer/stores/settings';
import { useSessionStore } from '@renderer/stores/session';
import Deck from '@renderer/components/deck/Deck.vue';
import Mixer from '@renderer/components/mixer/Mixer.vue';
import Browser from '@renderer/components/collection/Browser.vue';
import TopStrip from '@renderer/components/TopStrip.vue';
import EditView from '@renderer/components/deck/EditView.vue';
import SessionView from '@renderer/components/SessionView.vue';
import Modal from '@renderer/components/modals/Modal.vue';
import SettingsModal from '@renderer/components/Settings.vue';

const MIN_COLLECTION_H = 120;
const MAX_COLLECTION_H_RATIO = 0.65;

useKeyboard();

const decksStore = useDecksStore();
const collectionStore = useCollectionStore();
const mixerStore = useMixerStore();
const settingsStore = useSettingsStore();
const sessionStore = useSessionStore();
onMounted(() => settingsStore.init());
onUnmounted(() => decksStore.destroy());

const enterEditPending = ref(false);
const enterSessionPending = ref(false);
const exitSessionPending = ref(false);
const collectionHeight = ref(storageGet<number>(STORAGE_KEYS.collectionHeight, 200));

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

function tryToggleEditMode() {
  const entered = decksStore.tryToggleEditMode();
  if (!entered) enterEditPending.value = true;
}

function onConfirmEditMode() {
  enterEditPending.value = false;
  decksStore.enterEditMode();
}

function tryEnterSession() {
  const anyLoaded = ['A', 'B', 'C', 'D'].some(
    (id) => decksStore[`deck${id}` as 'deckA'].loadedPath != null
  );
  if (anyLoaded) {
    enterSessionPending.value = true;
  } else {
    sessionStore.enter();
  }
}

async function onConfirmEnterSession() {
  enterSessionPending.value = false;
  await sessionStore.ejectAllDecks();
  sessionStore.enter();
}

function tryExitSession() {
  if (sessionStore.session) {
    exitSessionPending.value = true;
  } else {
    sessionStore.exit();
  }
}

async function onConfirmExitSession() {
  exitSessionPending.value = false;
  await sessionStore.exit();
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

/* --collection-panel-h is driven by inline style when open */

.app__body {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  font-size: clamp(
    11px,
    calc((100dvh - var(--topstrip-h) - var(--collection-bar-h) - var(--collection-panel-h)) / 54),
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

.app__session-entry {
  display: flex;
  justify-content: flex-end;
  padding: 0 12px;
  height: 18px;
  flex-shrink: 0;
  align-items: center;
  border-top: 1px solid var(--color-border);
}

.app__session-btn {
  background: transparent;
  border: none;
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 9px;
  letter-spacing: 0.15em;
  cursor: pointer;
  padding: 0;
  opacity: 0.5;
}

.app__session-btn:hover {
  color: #06b6d4;
  opacity: 1;
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
  font-size: clamp(10px, 1vw, 12px);
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

.app__collection-resize-handle {
  height: 4px;
  flex-shrink: 0;
  cursor: ns-resize;
  background: var(--color-border);
  opacity: 0.4;
  transition: opacity 0.15s;
}
.app__collection-resize-handle:hover {
  opacity: 0.9;
}

.app__collection {
  width: 100%;
  height: var(--collection-panel-h);
  flex-shrink: 0;
  overflow: hidden;
  font-size: clamp(11px, 1.1vw, 14px);
}
</style>
