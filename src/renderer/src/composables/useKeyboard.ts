import { ref, onMounted, onUnmounted } from 'vue';
import { Deck, useDecksStore } from '@renderer/stores/decks';
import type { DeckId } from '@renderer/utils/types';
import { useCollectionStore } from '@renderer/stores/collection';
import { useMixerStore, FADER_GAIN, FILTER_ACTIVE } from '@renderer/stores/mixer';
import { useSettingsStore } from '@renderer/stores/settings';
import { useAppModeStore } from '@renderer/stores/appMode';
import { useSessionEditStore } from '@renderer/stores/sessionEdit';
import { useBrowseStore } from '@renderer/stores/browse';
import { commands, resolveKey, type Command } from '@renderer/keybindings';

export const shiftHeld = ref(false);

type DeckCommand = { deckId: 'A' | 'B' | 'C' | 'D'; command: Command } | null;

const DIGIT_DECK: Record<string, DeckId> = {
  Digit1: 'C',
  Digit2: 'A',
  Digit3: 'B',
  Digit4: 'D'
};

// While in swarm mode, swiping up/down moves the selected faders.
const SWARM_SWIPE_SENSITIVITY = 0.005;

// The browser is walked on the arrow cluster alone, so Enter stays free.
const BROWSE_KEYS = ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'];

export function useKeyboard() {
  const store = useDecksStore();
  const mixer = useMixerStore();
  const collection = useCollectionStore();
  const settings = useSettingsStore();
  const appMode = useAppModeStore();
  const sessionEdit = useSessionEditStore();
  const browse = useBrowseStore();

  // Space acts as a held modifier (Space+deck key = CUE) rather than arming swarm.
  const spaceHeld = ref(false);

  function anySwarmSelected(): boolean {
    return (Object.keys(mixer.swarmSelected) as DeckId[]).some((k) => mixer.swarmSelected[k]);
  }

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

  // A range input and a select step themselves on the arrow cluster, and every fader in
  // performance mode is a range, so taking the key stops a focused one responding.
  function stepsItselfOnArrows(target: EventTarget | null): boolean {
    if (target instanceof HTMLSelectElement) return true;
    return target instanceof HTMLInputElement && target.type.toLowerCase() === 'range';
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
      if (appMode.mode === 'performance') collection.toggleBigLibrary();
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

    // Ahead of the repeat filter: holding an arrow has to walk a long list rather
    // than step once. A user's own binding still wins, since arrows are capturable.
    if (
      appMode.mode === 'performance' &&
      !isTyping(e) &&
      BROWSE_KEYS.includes(e.key) &&
      !stepsItselfOnArrows(e.target) &&
      getDeckCommandFromKey(resolveKey(e)) === null
    ) {
      e.preventDefault();
      if (e.key === 'ArrowUp') browse.moveCursor(-1);
      else if (e.key === 'ArrowDown') browse.moveCursor(1);
      else if (e.key === 'ArrowLeft') browse.back();
      else browse.enter();
      return;
    }

    if (appMode.mode !== 'performance' || isTyping(e) || e.repeat) return;

    // Space is now just a modifier: while it's held, a deck key toggles that
    // channel's CUE; on its own a deck key selects it into the swarm.
    if (e.code === 'Space') {
      e.preventDefault();
      spaceHeld.value = true;
      return;
    }

    const digitDeck = DIGIT_DECK[e.code];
    if (digitDeck) {
      if (spaceHeld.value) {
        mixer.setCueActive(digitDeck, !mixer.cueActive[digitDeck]);
      } else if (e.shiftKey) {
        mixer.toggleParam(digitDeck, FILTER_ACTIVE);
      } else if (mixer.activeDecks.includes(digitDeck)) {
        mixer.setSwarmMode(true);
        mixer.setSwarmChannel(digitDeck, true);
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

    if (e.code === 'Space') {
      spaceHeld.value = false;
      return;
    }

    // Releasing a deck key always deselects it from the swarm, even if the app
    // mode changed or focus moved to a text input while it was held, so swarm
    // selection never gets stuck on. (A key that toggled CUE leaves swarmSelected
    // untouched, so this is a no-op for it.)
    const swarmDeck = DIGIT_DECK[e.code];
    if (swarmDeck) {
      if (mixer.swarmSelected[swarmDeck]) {
        mixer.setSwarmChannel(swarmDeck, false);
        if (!anySwarmSelected()) mixer.setSwarmMode(false);
      }
      return;
    }

    if (appMode.mode !== 'performance' || isTyping(e)) return;

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

  function onWheel(e: WheelEvent) {
    if (!mixer.swarmMode) return;
    e.preventDefault();
    const delta = e.deltaY * SWARM_SWIPE_SENSITIVITY;
    for (const deckId of Object.keys(mixer.swarmSelected) as DeckId[]) {
      if (mixer.swarmSelected[deckId]) {
        mixer.setParam(deckId, FADER_GAIN, mixer.paramValue(deckId, FADER_GAIN) + delta);
      }
    }
  }

  onMounted(() => {
    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
    window.addEventListener('wheel', onWheel, { passive: false });
  });

  onUnmounted(() => {
    window.removeEventListener('keydown', onKeyDown);
    window.removeEventListener('keyup', onKeyUp);
    window.removeEventListener('wheel', onWheel);
  });
}
