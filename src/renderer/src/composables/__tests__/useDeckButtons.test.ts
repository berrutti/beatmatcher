import { describe, it, expect, beforeEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import { useDeckButtons } from '@renderer/composables/useDeckButtons';
import { useAppModeStore } from '@renderer/stores/appMode';
import { DECKS_DISPOSITION, EDIT_DECK_ID } from '@renderer/stores/decks';

describe('the decks a track can be sent to', () => {
  beforeEach(() => setActivePinia(createPinia()));

  it('offers every live deck outside the edit view', () => {
    useAppModeStore().mode = 'performance';
    expect(useDeckButtons().deckIds.value).toEqual([...DECKS_DISPOSITION]);
  });

  it('offers the edit deck alone in the edit view', () => {
    useAppModeStore().mode = 'edit';
    expect(useDeckButtons().deckIds.value).toEqual([EDIT_DECK_ID]);
  });

  it('reserves less room for one button than for four', () => {
    const appMode = useAppModeStore();
    const { columnWidth } = useDeckButtons();

    appMode.mode = 'performance';
    const live = columnWidth.value;
    appMode.mode = 'edit';

    expect(columnWidth.value).toBeLessThan(live);
  });

  it('never shrinks below the header, which would wrap it', () => {
    const appMode = useAppModeStore();
    const { columnWidth } = useDeckButtons();

    for (const mode of ['performance', 'edit'] as const) {
      appMode.mode = mode;
      expect(columnWidth.value, mode).toBeGreaterThanOrEqual(72);
    }
  });
});
