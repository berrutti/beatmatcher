import { ref, onMounted, onUnmounted } from 'vue';
import { Deck, useDecksStore, type DeckId } from '@renderer/stores/decks';
import { useCollectionStore } from '@renderer/stores/collection';
import { useMixerStore } from '@renderer/stores/mixer';
import { KEYS, commands } from '@renderer/keybindings';

export const shiftHeld = ref(false);

type DeckName = keyof typeof KEYS;
type Command = (typeof commands)[keyof typeof commands];
type DeckCommand = { deckName: DeckName; command: Command } | null;

const CODE_TO_CHAR: Record<string, string> = {
  Period: '.',
  Comma: ',',
  Slash: '/',
  Semicolon: ';',
  Quote: "'",
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Minus: '-',
  Equal: '=',
  Backquote: '`'
};

function resolveKey(e: KeyboardEvent): string {
  if (e.code.startsWith('Key')) return e.code.slice(3).toLowerCase();
  return CODE_TO_CHAR[e.code] ?? e.key.toLowerCase();
}

function getDeckCommandFromKey(key: string): DeckCommand {
  for (const [deckName, bindings] of Object.entries(KEYS) as [
    DeckName,
    (typeof KEYS)[DeckName]
  ][]) {
    for (const command of Object.values(commands) as Command[]) {
      if (bindings[command] === key) {
        return {
          deckName,
          command
        };
      }
    }
  }

  return null;
}

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
        if (deck.playing) deck.stopAtCue();
        else deck.cueStart();
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
    mixer.setSwarmMode(e.getModifierState('CapsLock'));

    if (e.key === 'Tab') {
      e.preventDefault();
      collection.toggle();
      return;
    }

    if (e.key === 'Shift') {
      shiftHeld.value = true;
      return;
    }

    if (store.editMode || isTyping(e) || e.repeat) return;

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

    const deck = store[deckCommand.deckName];

    if (!deck) return;

    handleDeckCommand(deck, deckCommand.command, e.shiftKey);
  }

  function onKeyUp(e: KeyboardEvent) {
    mixer.setSwarmMode(e.getModifierState('CapsLock'));

    if (e.key === 'Shift') {
      shiftHeld.value = false;
      return;
    }

    if (store.editMode || isTyping(e)) return;

    if (mixer.swarmMode) {
      const digitDeck = DIGIT_DECK[e.code];
      if (digitDeck) {
        mixer.setSwarmChannel(digitDeck, false);
        return;
      }
    }

    const deckCommand = getDeckCommandFromKey(resolveKey(e));

    if (!deckCommand) return;

    const deck = store[deckCommand.deckName];

    if (!deck) return;

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
