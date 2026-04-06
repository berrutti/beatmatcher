<template>
  <div class="app">
    <div class="app__stage">
      <main class="app__decks">
        <DeckPanel :deck="store.deckA" :keybindings="KEYS.deckA" />
        <div class="app__center">
          <LissajousScope
            :sources="[
              { getPhase: () => store.deckA.phase, accent: store.deckA.accent, label: 'A' },
              { getPhase: () => store.deckB.phase, accent: store.deckB.accent, label: 'B' }
            ]"
          />
        </div>
        <DeckPanel :deck="store.deckB" :keybindings="KEYS.deckB" />
      </main>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onUnmounted } from 'vue';
import { useDecksStore } from '@renderer/stores/decks';
import { useKeyboard } from '@renderer/composables/useKeyboard';
import { KEYS } from '@renderer/keybindings';
import DeckPanel from '@renderer/components/DeckPanel.vue';
import LissajousScope from '@renderer/components/LissajousScope.vue';

useKeyboard();

const store = useDecksStore();
onUnmounted(() => store.destroy());
</script>

<style scoped>
.app {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.app__stage {
  position: relative;
  width: min(100vw, calc(100vh * 16 / 9));
  height: min(100vh, calc(100vw * 9 / 16));
  aspect-ratio: 16 / 9;
  overflow: hidden;
  font-size: calc(min(100vh, calc(100vw * 9 / 16)) / 45);
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
  align-items: center;
  justify-content: center;
  border-left: 1px solid var(--color-border);
  border-right: 1px solid var(--color-border);
}
</style>
