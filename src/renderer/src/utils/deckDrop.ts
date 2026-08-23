export type DeckDropDetail = {
  deckId: string;
  path: string;
  // A deck that will take the track calls this. The drag animates into the deck
  // only when one has, and returns the ghost to the list when none does.
  accept: () => void;
};

const DECK_DROP_EVENT = 'bm:collection-drop';

// Returns whether a deck took it, which the caller only learns because every
// handler runs before `dispatchEvent` comes back.
export function offerToDeck(path: string, deckId: string): boolean {
  let accepted = false;
  const detail: DeckDropDetail = {
    deckId,
    path,
    accept: () => {
      accepted = true;
    }
  };
  window.dispatchEvent(new CustomEvent(DECK_DROP_EVENT, { detail }));
  return accepted;
}

export function loadToDeck(path: string, deckId: string) {
  offerToDeck(path, deckId);
}
