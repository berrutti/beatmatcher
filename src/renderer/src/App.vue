<template>
  <div class="app" :class="{ 'app--collection-open': collectionStore.isOpen }">
    <div class="app__main">
      <div class="app__stage">
        <main class="app__decks">
          <DeckPanel :deck="store.deckA" :keybindings="KEYS.deckA" />
          <div class="app__center">
            <div class="app__scope">
              <LissajousScope
                :sources="[
                  { getPhase: () => store.deckA.phase, accent: store.deckA.accent, label: 'A' },
                  { getPhase: () => store.deckB.phase, accent: store.deckB.accent, label: 'B' }
                ]"
              />
            </div>
            <MixerPanel />
          </div>
          <DeckPanel :deck="store.deckB" :keybindings="KEYS.deckB" />
        </main>
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
import { onUnmounted } from 'vue';
import { useDecksStore } from '@renderer/stores/decks';
import { useCollectionStore } from '@renderer/stores/collection';
import { useKeyboard } from '@renderer/composables/useKeyboard';
import { KEYS } from '@renderer/keybindings';
import DeckPanel from '@renderer/components/DeckPanel.vue';
import LissajousScope from '@renderer/components/LissajousScope.vue';
import MixerPanel from '@renderer/components/MixerPanel.vue';
import CollectionPanel from '@renderer/components/CollectionPanel.vue';

useKeyboard();

const store = useDecksStore();
const collectionStore = useCollectionStore();
onUnmounted(() => store.destroy());
</script>

<style scoped>
.app {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  --collection-panel-h: 0px;
  --collection-bar-h: 22px;
}

.app--collection-open {
  --collection-panel-h: 200px;
}

.app__main {
  flex: 1;
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 0;
}

.app__stage {
  position: relative;
  width: min(100vw, calc((100vh - var(--collection-bar-h) - var(--collection-panel-h)) * 16 / 9));
  height: min(
    calc(100vh - var(--collection-bar-h) - var(--collection-panel-h)),
    calc(100vw * 9 / 16)
  );
  aspect-ratio: 16 / 9;
  overflow: hidden;
  font-size: calc(
    min(calc(100vh - var(--collection-bar-h) - var(--collection-panel-h)), calc(100vw * 9 / 16)) /
      45
  );
}

.app__decks {
  display: flex;
  width: 100%;
  height: 100%;
}

.app__center {
  width: 18em;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  border-left: 1px solid var(--color-border);
  border-right: 1px solid var(--color-border);
}

.app__scope {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
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
