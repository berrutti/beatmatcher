import { onMounted, onUnmounted } from 'vue';
import { useDecksStore } from '@renderer/stores/decks';
import { useCollectionStore } from '@renderer/stores/collection';
import { KEYS } from '@renderer/keybindings';

export function useKeyboard() {
  const store = useDecksStore();
  const collection = useCollectionStore();

  function isTyping(e: KeyboardEvent): boolean {
    const el = e.target as HTMLInputElement;
    if (el.tagName === 'TEXTAREA') return true;
    if (el.tagName === 'INPUT') {
      const type = el.type.toLowerCase();
      return type === 'text' || type === 'number' || type === 'email' || type === 'search';
    }
    return false;
  }

  function onKeyDown(e: KeyboardEvent) {
    if (isTyping(e)) return;
    if (e.repeat) return;

    if (e.key === 'Tab') {
      e.preventDefault();
      collection.toggle();
      return;
    }

    if (store.editMode) return;

    const { deckA, deckB } = store;
    const key = e.key.toUpperCase();

    if (key === KEYS.deckA.cue) {
      if (deckA.playing) deckA.stopAtCue();
      else deckA.cueStart();
      return;
    }
    if (key === KEYS.deckA.play) {
      deckA.togglePlay();
      return;
    }
    if (key === KEYS.deckA.nudgeBack) {
      deckA.nudgeStart('back');
      return;
    }
    if (key === KEYS.deckA.nudgeForward) {
      deckA.nudgeStart('forward');
      return;
    }
    if (key === KEYS.deckB.cue) {
      if (deckB.playing) deckB.stopAtCue();
      else deckB.cueStart();
      return;
    }
    if (key === KEYS.deckB.play) {
      deckB.togglePlay();
      return;
    }
    if (key === KEYS.deckB.nudgeBack) {
      deckB.nudgeStart('back');
      return;
    }
    if (key === KEYS.deckB.nudgeForward) {
      deckB.nudgeStart('forward');
      return;
    }
  }

  function onKeyUp(e: KeyboardEvent) {
    if (isTyping(e)) return;
    if (store.editMode) return;

    const { deckA, deckB } = store;
    const key = e.key.toUpperCase();

    if (key === KEYS.deckA.nudgeBack || key === KEYS.deckA.nudgeForward) deckA.nudgeEnd();
    if (key === KEYS.deckB.nudgeBack || key === KEYS.deckB.nudgeForward) deckB.nudgeEnd();
    if (key === KEYS.deckA.cue) deckA.cueEnd();
    if (key === KEYS.deckB.cue) deckB.cueEnd();
  }

  onMounted(() => {
    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
  });

  onUnmounted(() => {
    window.removeEventListener('keydown', onKeyDown);
    window.removeEventListener('keyup', onKeyUp);
  });
}
