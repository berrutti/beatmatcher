import { ref, onMounted, onUnmounted } from 'vue';
import { Deck, useDecksStore, type DeckId } from '@renderer/stores/decks';
import { useCollectionStore } from '@renderer/stores/collection';
import { useMixerStore } from '@renderer/stores/mixer';
import { useSettingsStore } from '@renderer/stores/settings';
import { useAppModeStore } from '@renderer/stores/appMode';
import { useSessionEditStore } from '@renderer/stores/sessionEdit';
import { commands, resolveKey, type Command } from '@renderer/keybindings';

export const shiftHeld = ref(false);

type DeckCommand = { deckId: 'A' | 'B' | 'C' | 'D'; command: Command } | null;

const DIGIT_DECK: Record<string, DeckId> = {
  Digit1: 'C',
  Digit2: 'A',
  Digit3: 'B',
  Digit4: 'D'
};

export function useKeyboard() {
  const store = useDecksStore();
  const mixer = useMixerStore();
  const collection = useCollectionStore();
  const settings = useSettingsStore();
  const appMode = useAppModeStore();
  const sessionEdit = useSessionEditStore();

  function getDeckCommandFromKey(key: string): DeckCommand {
    for (const [deckId, bindings] of Object.entries(settings.keybindings) as [
      'A' | 'B' | 'C' | 'D',
      Record<Command, string>
    ][]) {
      for (const command of Object.values(commands) as Command[]) {
        if (bindings[command] === key) return { deckId, command };
      }
    }
    return null;
  }

  function isTyping(e: KeyboardEvent): boolean {
    const el = e.target as HTMLInputElement;
    if (!el?.tagName) return false;
    if (el.tagName === 'TEXTAREA') return true;
    if (el.tagName === 'INPUT') {
      const type = el.type.toLowerCase();
      return type === 'text' || type === 'number' || type === 'email' || type === 'search';
    }
    return false;
  }

  function handleDeckCommand(deck: Deck, command: Command, shiftKey: boolean) {
    switch (command) {
      case commands.CUE:
        deck.cueStart();
        break;
      case commands.PLAY:
        deck.togglePlay();
        break;
      case commands.NUDGE_BACK:
        deck.nudgeStart('back');
        break;
      case commands.NUDGE_FORWARD:
        deck.nudgeStart('forward');
        break;
      case commands.LOOP_IN:
        deck.setLoopIn();
        break;
      case commands.LOOP_OUT_EXIT:
        if (shiftKey) {
          if (deck.loopActive) deck.exitLoop();
          else if (deck.loopRegion) deck.reloop();
        } else {
          deck.setLoopOut();
        }
        break;
    }
  }

  function onKeyDown(e: KeyboardEvent) {
    if (settings.isOpen) return;

    if (e.key === ',' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      settings.isOpen = true;
      return;
    }

    if (e.key === 'Tab') {
      e.preventDefault();
      collection.toggle();
      return;
    }

    if (e.key === 'Shift') {
      shiftHeld.value = true;
      return;
    }

    if (appMode.mode === 'session' && !isTyping(e)) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'z') {
        e.preventDefault();
        if (e.shiftKey) sessionEdit.redo();
        else sessionEdit.undo();
        return;
      }
      if (e.key === 'Escape' && sessionEdit.selectedLane) {
        sessionEdit.selectedLane = null;
        return;
      }
    }

    // Spacebar toggles the edit deck's transport. Edit mode only: in
    // performance mode space must never touch playback, and session mode has
    // its own handler in Session.vue (the playhead state lives there).
    if (appMode.mode === 'edit' && !isTyping(e) && e.code === 'Space' && !e.repeat) {
      e.preventDefault();
      store.decks.E?.togglePlay().catch(() => {});
      return;
    }

    if (appMode.mode !== 'performance' || isTyping(e) || e.repeat) return;

    mixer.setSwarmMode(e.getModifierState('CapsLock'));

    const digitDeck = DIGIT_DECK[e.code];
    if (digitDeck) {
      if (mixer.swarmMode) {
        mixer.setSwarmChannel(digitDeck, true);
      } else if (e.shiftKey) {
        mixer.toggleFilter(digitDeck);
      } else {
        mixer.setCueActive(digitDeck, !mixer.cueActive[digitDeck]);
      }
      return;
    }

    const deckCommand = getDeckCommandFromKey(resolveKey(e));
    if (!deckCommand) return;

    const deck = store.decks[deckCommand.deckId];
    if (!deck || !deck.acceptsCommands) return;

    handleDeckCommand(deck, deckCommand.command, e.shiftKey);
  }

  function onKeyUp(e: KeyboardEvent) {
    if (settings.isOpen) return;

    if (e.key === 'Shift') {
      shiftHeld.value = false;
      return;
    }

    if (appMode.mode !== 'performance' || isTyping(e)) return;

    mixer.setSwarmMode(e.getModifierState('CapsLock'));

    if (mixer.swarmMode) {
      const digitDeck = DIGIT_DECK[e.code];
      if (digitDeck) {
        mixer.setSwarmChannel(digitDeck, false);
        return;
      }
    }

    const deckCommand = getDeckCommandFromKey(resolveKey(e));
    if (!deckCommand) return;

    const deck = store.decks[deckCommand.deckId];
    if (!deck || !deck.acceptsCommands) return;

    if (
      deckCommand.command === commands.NUDGE_BACK ||
      deckCommand.command === commands.NUDGE_FORWARD
    ) {
      deck.nudgeEnd();
    }

    if (deckCommand.command === commands.CUE) {
      deck.cueEnd();
    }
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
