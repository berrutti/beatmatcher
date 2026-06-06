<template>
  <div class="deck-buttons">
    <template v-if="appModeStore.mode === 'edit'">
      <button
        class="deck-btn"
        :class="{ 'deck-btn--loaded': deckLoaded('E') }"
        :style="{ '--btn-color': decksStore.deckE.accent }"
        :disabled="disabled || deckLoaded('E')"
        tabindex="-1"
        title="Click to send to Edit"
        @click.stop="load('E')"
      >
        {{ decksStore.decks['E'].name }}
      </button>
    </template>
    <template v-else>
      <button
        v-for="deckId in DECKS_DISPOSITION"
        :key="deckId"
        class="deck-btn"
        :class="{ 'deck-btn--loaded': deckLoaded(deckId) }"
        :style="{ '--btn-color': decksStore.decks[deckId].accent }"
        :disabled="disabled || deckLoaded(deckId)"
        tabindex="-1"
        :title="`Click to send to Deck ${deckId}`"
        @click.stop="load(deckId)"
      >
        {{ decksStore.decks[deckId].name }}
      </button>
    </template>
  </div>
</template>

<script setup lang="ts">
import { useDecksStore, DECKS_DISPOSITION } from '@renderer/stores/decks';
import type { DeckId } from '@renderer/stores/decks';
import { useAppModeStore } from '@renderer/stores/appMode';

const props = defineProps<{
  path: string;
  disabled?: boolean;
}>();

const decksStore = useDecksStore();
const appModeStore = useAppModeStore();

function deckLoaded(deckId: string): boolean {
  return decksStore.decks[deckId as DeckId].loadedPath === props.path;
}

function load(deckId: string) {
  window.dispatchEvent(
    new CustomEvent('bm:collection-drop', { detail: { deckId, path: props.path } })
  );
}
</script>

<style scoped>
.deck-buttons {
  display: flex;
  gap: 3px;
  flex-shrink: 0;
  align-self: stretch;
  align-items: stretch;
  padding: 3px 0;
}

.deck-btn {
  padding: 0 0.5em;
  border: 1px solid var(--btn-color);
  color: var(--btn-color);
  background: transparent;
  font-family: var(--font);
  font-size: 0.8em;
  font-weight: 700;
  border-radius: 2px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  white-space: nowrap;
  flex-shrink: 0;
  transition: background 0.1s;
}

.deck-btn:hover:not(:disabled) {
  background: color-mix(in srgb, var(--btn-color) 20%, transparent);
}

.deck-btn--loaded {
  background: color-mix(in srgb, var(--btn-color) 25%, transparent);
  cursor: default;
}

.deck-btn:disabled {
  opacity: 0.35;
  cursor: default;
}
</style>
