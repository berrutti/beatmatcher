import { ref, onMounted, onUnmounted } from 'vue';
import type { LoadableTrack } from '@renderer/stores/decks';
import { DECK_DROP_EVENT, type DeckDropDetail } from '@renderer/utils/deckDrop';
import { DROP_LANDING_MS } from '@renderer/utils/dragGhostLanding';

// What a drop target needs from a deck, so a caller passes what it has rather
// than the whole store.
export type DropTargetDeck = {
  id: string;
  loadedPath: string | null;
  loopPlaying: boolean;
  loadTrack: (track: LoadableTrack) => Promise<void>;
};

export type DeckDropOptions = {
  deck: () => DropTargetDeck;
  resolve: (path: string) => LoadableTrack | null;
};

// A target that loads a track without accepting the offer sends the drag ghost
// home while the track loads anyway.
export function useDeckDrop({ deck, resolve }: DeckDropOptions) {
  const pendingLoad = ref<LoadableTrack | null>(null);
  let scheduledLoad = 0;

  // Held for the length of the drop animation, so the deck takes the track as
  // the ghost reaches it rather than the instant the pointer came up.
  function scheduleLoad(loadable: LoadableTrack) {
    clearScheduledLoad();
    scheduledLoad = window.setTimeout(async () => {
      scheduledLoad = 0;
      try {
        await deck().loadTrack(loadable);
      } catch (error) {
        console.error('deck load failed', error);
      }
    }, DROP_LANDING_MS);
  }

  function clearScheduledLoad() {
    if (scheduledLoad !== 0) window.clearTimeout(scheduledLoad);
    scheduledLoad = 0;
  }

  function onCollectionDrop(event: Event) {
    if (!(event instanceof CustomEvent)) return;
    const detail: DeckDropDetail = event.detail;
    const target = deck();
    if (detail.deckId !== target.id) return;
    if (target.loadedPath === detail.path) return;
    const loadable = resolve(detail.path);
    if (!loadable) return;
    detail.accept();
    if (target.loopPlaying) {
      pendingLoad.value = loadable;
      return;
    }
    scheduleLoad(loadable);
  }

  // Already confirmed by the user, so it loads now rather than waiting on an
  // animation that finished while the dialog was up.
  function confirmPendingLoad() {
    const loadable = pendingLoad.value;
    pendingLoad.value = null;
    if (loadable)
      deck()
        .loadTrack(loadable)
        .catch(() => {});
  }

  function cancelPendingLoad() {
    pendingLoad.value = null;
  }

  onMounted(() => window.addEventListener(DECK_DROP_EVENT, onCollectionDrop));
  onUnmounted(() => {
    window.removeEventListener(DECK_DROP_EVENT, onCollectionDrop);
    clearScheduledLoad();
  });

  return { pendingLoad, confirmPendingLoad, cancelPendingLoad };
}
