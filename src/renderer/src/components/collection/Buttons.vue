<template>
  <div class="deck-buttons">
    <template v-if="appModeStore.mode === 'edit'">
      <button
        class="deck-btn"
        :class="{ 'deck-btn--loaded': deckLoaded('E'), 'deck-btn--unavailable': disabled }"
        :style="{ '--btn-color': decksStore.deckE.accent }"
        :disabled="disabled || deckLoaded('E')"
        tabindex="-1"
        v-tooltip="
          deckLoaded('E')
            ? $t('browser.sameDeck')
            : disabled
              ? unavailableTooltip
              : $t('browser.sendToEdit')
        "
        @click.stop="loadToDeck(path, 'E')"
      >
        {{ decksStore.decks['E'].name }}
      </button>
    </template>
    <template v-else>
      <button
        v-for="deckId in DECKS_DISPOSITION"
        :key="deckId"
        class="deck-btn"
        :class="{ 'deck-btn--loaded': deckLoaded(deckId), 'deck-btn--unavailable': disabled }"
        :style="{ '--btn-color': decksStore.decks[deckId].accent }"
        :disabled="disabled || deckLoaded(deckId)"
        tabindex="-1"
        v-tooltip="
          deckLoaded(deckId)
            ? $t('browser.sameDeck')
            : disabled
              ? unavailableTooltip
              : $t('browser.sendToDeck', { deckId })
        "
        @click.stop="loadToDeck(path, deckId)"
      >
        {{ decksStore.decks[deckId].name }}
      </button>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { useDecksStore, DECKS_DISPOSITION } from '@renderer/stores/decks';
import { loadToDeck } from '@renderer/utils/deckDrop';
import type { DeckId } from '@renderer/utils/types';
import { useAppModeStore } from '@renderer/stores/appMode';

const props = defineProps<{
  path: string;
  disabled?: boolean;
  unavailableTooltip?: string;
}>();

const { t } = useI18n();

const unavailableTooltip = computed(() => props.unavailableTooltip ?? t('browser.analyzeFirst'));

const decksStore = useDecksStore();
const appModeStore = useAppModeStore();

function deckLoaded(deckId: DeckId): boolean {
  return decksStore.decks[deckId].loadedPath === props.path;
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
  opacity: var(--disabled-opacity);
  cursor: default;
}

.deck-btn--unavailable {
  border-style: dashed;
  cursor: not-allowed;
}
</style>
