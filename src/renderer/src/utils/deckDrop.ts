export function loadToDeck(path: string, deckId: string) {
  window.dispatchEvent(new CustomEvent('bm:collection-drop', { detail: { deckId, path } }));
}
