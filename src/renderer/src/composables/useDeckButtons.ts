import { computed } from 'vue';
import { DECKS_DISPOSITION, EDIT_DECK_ID } from '@renderer/stores/decks';
import { useAppModeStore } from '@renderer/stores/appMode';
import type { DeckId } from '@renderer/utils/types';

// Sized for a "DECK A" label, which is what the buttons carry.
const BUTTON_WIDTH_PX = 44;
const BUTTON_GAP_PX = 3;
// The header still has to fit on one line when a single button would not fill it.
const HEADER_WIDTH_PX = 72;

export function useDeckButtons() {
  const appMode = useAppModeStore();
  const deckIds = computed<DeckId[]>(() =>
    appMode.mode === 'edit' ? [EDIT_DECK_ID] : [...DECKS_DISPOSITION]
  );
  const columnWidth = computed(() => {
    const buttons = deckIds.value.length;
    return Math.max(HEADER_WIDTH_PX, buttons * BUTTON_WIDTH_PX + (buttons - 1) * BUTTON_GAP_PX);
  });
  return { deckIds, columnWidth };
}
